use std::{
    error::Error,
    io::{self, Stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use reqwest::{Client, Method};
use starforge_api::{
    CreateSessionRequest, JoinSessionRequest, PlayerAlert, PlayerCommandRequest,
    PlayerFrameResponse, ReadySessionRequest, RunnerSpeedRequest, SessionInfoResponse, SessionMode,
    SessionPhase,
};
use starforge_core::{
    CommandKind, LocationVisibility, PlayerId, RelayStatus, ResearchBranch, SessionId,
};
use tokio::{sync::mpsc, time::sleep};

use crate::{
    cli::{PlayerScopedCommand, parse_player_scoped_command},
    render::{
        format_stockpiles, render_collapse, render_event, render_location, render_map_lines,
        render_research_branch, render_status_lines,
    },
};

type DynError = Box<dyn Error>;

pub(crate) async fn create_and_play(
    api_url: String,
    player_id: PlayerId,
    mode: SessionMode,
) -> Result<(), DynError> {
    let client = ApiClient::new(api_url);
    let response = client
        .create_session(CreateSessionRequest {
            mode,
            claimed_player_id: Some(player_id),
        })
        .await?;
    run_tui(client, response.session_id, player_id).await
}

pub(crate) async fn join_and_play(
    api_url: String,
    session_id: SessionId,
    player_id: PlayerId,
) -> Result<(), DynError> {
    let client = ApiClient::new(api_url);
    client.join_session(session_id, player_id).await?;
    run_tui(client, session_id, player_id).await
}

async fn run_tui(
    client: ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
) -> Result<(), DynError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let stop = Arc::new(AtomicBool::new(false));
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let event_thread = spawn_event_thread(event_tx, Arc::clone(&stop));

    let result = tui_loop(&client, session_id, player_id, &mut terminal, &mut event_rx).await;

    stop.store(true, Ordering::SeqCst);
    let _ = event_thread.join();
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

async fn tui_loop(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    event_rx: &mut mpsc::UnboundedReceiver<Event>,
) -> Result<(), DynError> {
    let mut frame = client.frame(session_id, player_id, 0).await?;
    let mut state = TuiState::from_frame(&frame);

    loop {
        terminal.draw(|ui| draw(ui, &frame, &state, player_id))?;

        tokio::select! {
            maybe_event = event_rx.recv() => {
                let Some(event) = maybe_event else { break; };
                match handle_event(&event, &mut state, &frame, player_id) {
                    Ok(Some(UiCommand::Quit)) => break,
                    Ok(Some(command)) => {
                        if let Err(error) = apply_ui_command(
                            client,
                            session_id,
                            player_id,
                            &mut state,
                            &mut frame,
                            command,
                        )
                        .await
                        {
                            report_ui_error(&mut state, &*error);
                            try_refresh_frame(
                                client,
                                session_id,
                                player_id,
                                &mut state,
                                &mut frame,
                            )
                            .await;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => report_ui_error(&mut state, &*error),
                }
            }
            _ = sleep(Duration::from_millis(100)) => {
                let updated = client.frame(session_id, player_id, frame.next_event_index).await?;
                state.apply_frame(&updated);
                frame = updated;
            }
        }
    }

    Ok(())
}

fn report_ui_error(state: &mut TuiState, error: &dyn Error) {
    state.push_output(format!("Error: {error}"));
}

async fn try_refresh_frame(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
) {
    match client
        .frame(session_id, player_id, frame.next_event_index)
        .await
    {
        Ok(updated) => {
            state.apply_frame(&updated);
            *frame = updated;
        }
        Err(error) => report_ui_error(state, &*error),
    }
}

fn spawn_event_thread(
    sender: mpsc::UnboundedSender<Event>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            if event::poll(Duration::from_millis(50)).unwrap_or(false)
                && let Ok(event) = event::read()
            {
                let _ = sender.send(event);
            }
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FocusPane {
    Map,
    Overview,
    Events,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalState {
    CommandLine { input: String },
    Build { input: String },
    Repair { input: String },
    Budget { input: String },
    Research { input: String },
    Train { input: String },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiState {
    pub(crate) selected_location_index: usize,
    pub(crate) origin_location_id: Option<u32>,
    pub(crate) target_location_id: Option<u32>,
    pub(crate) focus: FocusPane,
    pub(crate) modal: Option<ModalState>,
    pub(crate) unread_alerts: Vec<PlayerAlert>,
    recent_events: Vec<String>,
    output_lines: Vec<String>,
}

impl TuiState {
    pub(crate) fn from_frame(frame: &PlayerFrameResponse) -> Self {
        let mut state = Self {
            selected_location_index: 0,
            origin_location_id: None,
            target_location_id: None,
            focus: FocusPane::Overview,
            modal: None,
            unread_alerts: Vec::new(),
            recent_events: Vec::new(),
            output_lines: vec!["Connected to live session".to_owned()],
        };
        state.apply_frame(frame);
        state
    }

    pub(crate) fn apply_frame(&mut self, frame: &PlayerFrameResponse) {
        if frame.view.locations.is_empty() {
            self.selected_location_index = 0;
            self.target_location_id = None;
            self.origin_location_id = None;
        } else if self.selected_location_index >= frame.view.locations.len() {
            self.selected_location_index = frame.view.locations.len().saturating_sub(1);
        }

        if let Some(origin) = self.origin_location_id
            && !frame
                .view
                .locations
                .iter()
                .any(|location| location.location_id == origin)
        {
            self.origin_location_id = None;
        }
        if let Some(target) = self.target_location_id
            && !frame
                .view
                .locations
                .iter()
                .any(|location| location.location_id == target)
        {
            self.target_location_id = None;
        }

        if !frame.events.is_empty() {
            self.recent_events.extend(frame.events.iter().map(|event| {
                format!(
                    "[{}] {}",
                    event.record.tick_id.0,
                    render_event(&event.record.kind)
                )
            }));
            if self.recent_events.len() > 40 {
                let drop_count = self.recent_events.len() - 40;
                self.recent_events.drain(..drop_count);
            }
        }

        if !frame.alerts.is_empty() {
            self.unread_alerts.extend(frame.alerts.clone());
            if self.unread_alerts.len() > 20 {
                let drop_count = self.unread_alerts.len() - 20;
                self.unread_alerts.drain(..drop_count);
            }
            if let Some(location_id) = frame.alerts.iter().find_map(|alert| alert.location_id) {
                self.target_location_id = Some(location_id);
                if let Some(index) = frame
                    .view
                    .locations
                    .iter()
                    .position(|location| location.location_id == location_id)
                {
                    self.selected_location_index = index;
                }
            }
        }
    }

    pub(crate) fn select_next(&mut self, location_count: usize) {
        if location_count == 0 {
            self.selected_location_index = 0;
            return;
        }

        self.selected_location_index = (self.selected_location_index + 1) % location_count;
    }

    pub(crate) fn select_previous(&mut self, location_count: usize) {
        if location_count == 0 {
            self.selected_location_index = 0;
            return;
        }

        self.selected_location_index =
            (self.selected_location_index + location_count - 1) % location_count;
    }

    pub(crate) fn set_origin_from_selection(
        &mut self,
        frame: &PlayerFrameResponse,
        player_id: PlayerId,
    ) {
        let Some(location) = frame.view.locations.get(self.selected_location_index) else {
            return;
        };
        if location.controller == Some(player_id)
            && location.visibility == LocationVisibility::Owned
        {
            self.origin_location_id = Some(location.location_id);
            self.push_output(format!("Origin set to #{}", location.location_id));
        }
    }

    pub(crate) fn set_target_from_selection(&mut self, frame: &PlayerFrameResponse) {
        let Some(location) = frame.view.locations.get(self.selected_location_index) else {
            return;
        };
        self.target_location_id = Some(location.location_id);
        self.push_output(format!("Target set to #{}", location.location_id));
    }

    pub(crate) fn open_modal(&mut self, modal: ModalState) {
        self.modal = Some(modal);
    }

    pub(crate) fn acknowledge_alerts(&mut self) {
        self.unread_alerts.clear();
    }

    fn push_output(&mut self, line: String) {
        self.output_lines.push(line);
        if self.output_lines.len() > 8 {
            let drop_count = self.output_lines.len() - 8;
            self.output_lines.drain(..drop_count);
        }
    }
}

enum UiCommand {
    Quit,
    ExecuteParsed(PlayerScopedCommand),
    RefreshAllEvents(u64),
    ToggleReady,
    TogglePause,
    SetSpeed(u64),
    ToggleRelay,
    QuickCommand(PlayerScopedCommand),
}

fn handle_event(
    event: &Event,
    state: &mut TuiState,
    frame: &PlayerFrameResponse,
    player_id: PlayerId,
) -> Result<Option<UiCommand>, DynError> {
    let Event::Key(key) = event else {
        return Ok(None);
    };

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(Some(UiCommand::Quit));
    }

    if let Some(modal) = state.modal.take() {
        let (command, next_modal) = handle_modal_key(key, modal)?;
        state.modal = next_modal;
        return Ok(command);
    }

    let location_count = frame.view.locations.len();
    match key.code {
        KeyCode::Char('q') => Ok(Some(UiCommand::Quit)),
        KeyCode::Up => {
            state.select_previous(location_count);
            Ok(None)
        }
        KeyCode::Down => {
            state.select_next(location_count);
            Ok(None)
        }
        KeyCode::Char('o') => {
            state.set_origin_from_selection(frame, player_id);
            Ok(None)
        }
        KeyCode::Enter => {
            state.set_target_from_selection(frame);
            Ok(None)
        }
        KeyCode::Char('s') => quick_hotkey_command(state, PlayerScopedCommand::Survey),
        KeyCode::Char('p') => quick_hotkey_command(state, PlayerScopedCommand::Pacify),
        KeyCode::Char('c') => quick_hotkey_command(state, PlayerScopedCommand::Claim),
        KeyCode::Char('a') => quick_hotkey_command(state, PlayerScopedCommand::Assault),
        KeyCode::Char('x') => quick_hotkey_command(state, PlayerScopedCommand::Strike),
        KeyCode::Char('b') => {
            if let Some(location_id) = selected_location_id(frame, state) {
                state.open_modal(ModalState::Build {
                    input: format!("build --location {} --kind ", location_id),
                });
            }
            Ok(None)
        }
        KeyCode::Char('r') => {
            if let Some(location_id) = selected_location_id(frame, state) {
                state.open_modal(ModalState::Repair {
                    input: format!("repair --location {} --kind ", location_id),
                });
            }
            Ok(None)
        }
        KeyCode::Char('u') => {
            state.open_modal(ModalState::Budget {
                input: format!(
                    "budget --upkeep {} --research {} --training {} --agents {}",
                    frame.view.throughput.reserved_for_model_upkeep,
                    frame.view.throughput.reserved_for_research,
                    frame.view.throughput.reserved_for_training,
                    frame.view.throughput.reserved_for_agents
                ),
            });
            Ok(None)
        }
        KeyCode::Char('g') => {
            state.open_modal(ModalState::Research {
                input: default_research_command(frame),
            });
            Ok(None)
        }
        KeyCode::Char('t') => {
            let next_tier = frame.view.model_tier.saturating_add(1);
            state.open_modal(ModalState::Train {
                input: format!("train --target-tier {}", next_tier),
            });
            Ok(None)
        }
        KeyCode::Char('l') => Ok(Some(UiCommand::ToggleRelay)),
        KeyCode::Char('m') => {
            state.focus = FocusPane::Map;
            Ok(None)
        }
        KeyCode::Char('e') => {
            state.focus = FocusPane::Events;
            Ok(None)
        }
        KeyCode::Char('i') => {
            state.focus = FocusPane::Overview;
            Ok(None)
        }
        KeyCode::Char(':') => {
            state.open_modal(ModalState::CommandLine {
                input: String::new(),
            });
            Ok(None)
        }
        KeyCode::Char('?') => {
            state.open_modal(ModalState::Help);
            Ok(None)
        }
        KeyCode::Esc => {
            state.origin_location_id = None;
            state.target_location_id = None;
            state.acknowledge_alerts();
            Ok(None)
        }
        KeyCode::Char('R') => Ok(Some(UiCommand::ToggleReady)),
        KeyCode::Char(' ') => Ok(Some(UiCommand::TogglePause)),
        KeyCode::Char('1') => Ok(Some(UiCommand::SetSpeed(500))),
        KeyCode::Char('2') => Ok(Some(UiCommand::SetSpeed(250))),
        KeyCode::Char('3') => Ok(Some(UiCommand::SetSpeed(125))),
        _ => Ok(None),
    }
}

fn handle_modal_key(
    key: &KeyEvent,
    mut modal: ModalState,
) -> Result<(Option<UiCommand>, Option<ModalState>), DynError> {
    match &mut modal {
        ModalState::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => Ok((None, None)),
            _ => Ok((None, Some(modal))),
        },
        ModalState::CommandLine { input }
        | ModalState::Build { input }
        | ModalState::Repair { input }
        | ModalState::Budget { input }
        | ModalState::Research { input }
        | ModalState::Train { input } => match key.code {
            KeyCode::Esc => Ok((None, None)),
            KeyCode::Backspace => {
                input.pop();
                Ok((None, Some(modal)))
            }
            KeyCode::Enter => {
                let command_text = input.trim().to_owned();
                if command_text.is_empty() {
                    return Ok((None, None));
                }
                let command = parse_player_scoped_command(&command_text)?;
                match command {
                    PlayerScopedCommand::Events(args) => {
                        Ok((Some(UiCommand::RefreshAllEvents(args.from_tick)), None))
                    }
                    parsed => Ok((Some(UiCommand::ExecuteParsed(parsed)), None)),
                }
            }
            KeyCode::Char(character) => {
                input.push(character);
                Ok((None, Some(modal)))
            }
            _ => Ok((None, Some(modal))),
        },
    }
}

fn build_quick_command(
    state: &TuiState,
    builder: fn(crate::cli::TransitSpec) -> PlayerScopedCommand,
) -> Option<UiCommand> {
    let (Some(origin), Some(target)) = (state.origin_location_id, state.target_location_id) else {
        return None;
    };

    Some(UiCommand::QuickCommand(builder(crate::cli::TransitSpec {
        origin,
        destination: target,
    })))
}

fn quick_hotkey_command(
    state: &mut TuiState,
    builder: fn(crate::cli::TransitSpec) -> PlayerScopedCommand,
) -> Result<Option<UiCommand>, DynError> {
    if let Some(command) = build_quick_command(state, builder) {
        Ok(Some(command))
    } else {
        state.push_output(
            "Set both an origin and a target before using expedition hotkeys".to_owned(),
        );
        Ok(None)
    }
}

fn selected_location_id(frame: &PlayerFrameResponse, state: &TuiState) -> Option<u32> {
    frame
        .view
        .locations
        .get(state.selected_location_index)
        .map(|location| location.location_id)
}

fn default_research_command(frame: &PlayerFrameResponse) -> String {
    let candidates = [
        (ResearchBranch::Industry, frame.view.research.industry_level),
        (ResearchBranch::Models, frame.view.research.models_level),
        (ResearchBranch::Warfare, frame.view.research.warfare_level),
        (
            ResearchBranch::Resilience,
            frame.view.research.resilience_level,
        ),
    ];
    let (branch, current_level) = candidates
        .into_iter()
        .min_by_key(|(_, level)| *level)
        .expect("research branches should always be available");

    format!(
        "research --branch {} --target-level {}",
        render_research_branch(branch),
        current_level.saturating_add(1)
    )
}

async fn apply_ui_command(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
    command: UiCommand,
) -> Result<(), DynError> {
    match command {
        UiCommand::Quit => {}
        UiCommand::ExecuteParsed(parsed) | UiCommand::QuickCommand(parsed) => {
            execute_player_scoped_command(client, session_id, player_id, state, frame, parsed)
                .await?;
        }
        UiCommand::RefreshAllEvents(from_tick) => {
            let full_frame = client.frame(session_id, player_id, 0).await?;
            state.push_output(format!("Visible events from tick {from_tick}:"));
            let filtered = full_frame
                .events
                .iter()
                .filter(|event| event.record.tick_id.0 >= from_tick)
                .map(|event| {
                    format!(
                        "[{}] {}",
                        event.record.tick_id.0,
                        render_event(&event.record.kind)
                    )
                })
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                state.push_output(format!("No visible events from tick {from_tick}"));
            } else {
                for line in filtered.into_iter().take(6) {
                    state.push_output(line);
                }
            }
            state.apply_frame(&full_frame);
            *frame = full_frame;
        }
        UiCommand::ToggleReady => {
            if frame.runner.phase == SessionPhase::Lobby {
                let is_ready = frame
                    .seats
                    .iter()
                    .find(|seat| seat.player_id == player_id)
                    .map(|seat| seat.ready)
                    .unwrap_or(false);
                client.set_ready(session_id, player_id, !is_ready).await?;
                state.push_output(if is_ready {
                    "Marked not ready".to_owned()
                } else {
                    "Marked ready".to_owned()
                });
                let updated = client
                    .frame(session_id, player_id, frame.next_event_index)
                    .await?;
                state.apply_frame(&updated);
                *frame = updated;
            }
        }
        UiCommand::TogglePause => {
            if frame.runner.mode == SessionMode::Sandbox {
                if frame.runner.paused {
                    client.run_session(session_id).await?;
                    state.push_output("Sandbox session resumed".to_owned());
                } else {
                    client.pause_session(session_id).await?;
                    state.push_output("Sandbox session paused".to_owned());
                }
                let updated = client
                    .frame(session_id, player_id, frame.next_event_index)
                    .await?;
                state.apply_frame(&updated);
                *frame = updated;
            }
        }
        UiCommand::SetSpeed(speed) => {
            if frame.runner.mode == SessionMode::Sandbox {
                client.set_speed(session_id, speed).await?;
                state.push_output(format!("Tick speed set to {} ms", speed));
                let updated = client
                    .frame(session_id, player_id, frame.next_event_index)
                    .await?;
                state.apply_frame(&updated);
                *frame = updated;
            }
        }
        UiCommand::ToggleRelay => {
            if let Some(location) = frame.view.locations.get(state.selected_location_index)
                && let Some(relay_status) = &location.relay_status
            {
                let new_status = match relay_status {
                    RelayStatus::Connected => RelayStatus::Disconnected,
                    RelayStatus::Disconnected => RelayStatus::Connected,
                };
                execute_player_scoped_command(
                    client,
                    session_id,
                    player_id,
                    state,
                    frame,
                    PlayerScopedCommand::Relay(crate::cli::ScopedRelayArgs {
                        location: location.location_id,
                        status: new_status,
                    }),
                )
                .await?;
            }
        }
    }

    Ok(())
}

async fn execute_player_scoped_command(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
    command: PlayerScopedCommand,
) -> Result<(), DynError> {
    match &command {
        PlayerScopedCommand::Status => {
            for line in render_status_lines(
                &format!("Live session: {}", frame.session_id.0),
                player_id,
                &frame.view,
                &frame.summary.victory,
            )
            .into_iter()
            .take(6)
            {
                state.push_output(line);
            }
        }
        PlayerScopedCommand::Map => {
            for line in render_map_lines(
                player_id,
                frame.summary.current_tick.0,
                &frame.summary.victory,
                &frame.view.locations,
                &frame.known_routes,
            )
            .into_iter()
            .take(6)
            {
                state.push_output(line);
            }
        }
        PlayerScopedCommand::Events(_) => {}
        _ => {
            let command_kind = command
                .to_command_kind()
                .expect("mutating commands should produce command kinds");
            client
                .issue_command(session_id, player_id, command_kind)
                .await?;
            if let Some(message) = command.success_message() {
                state.push_output(message.to_owned());
            }
            let updated = client
                .frame(session_id, player_id, frame.next_event_index)
                .await?;
            state.apply_frame(&updated);
            *frame = updated;
        }
    }

    Ok(())
}

fn draw(
    frame: &mut ratatui::Frame<'_>,
    live_frame: &PlayerFrameResponse,
    state: &TuiState,
    player_id: PlayerId,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(10),
            Constraint::Length(6),
        ])
        .split(frame.area());

    let top_lines = live_status_lines(live_frame, player_id, state);
    let status = Paragraph::new(top_lines.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    frame.render_widget(status, layout[0]);

    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(layout[1]);

    draw_location_list(frame, live_frame, state, middle[0]);
    draw_details(frame, live_frame, state, middle[1]);
    draw_events(frame, state, middle[2]);

    let bottom_text = [
        "Hotkeys: Up/Down move  Enter target  o origin  s/p/c/a/x actions  b/r/u/g/t command modals  l relay  R ready  Space/1/2/3 sandbox  : command  ? help  q quit".to_owned(),
        format!(
            "Focus: {:?}  Origin: {:?}  Target: {:?}  Alerts: {}  Runner: {:?} paused={} speed={}ms",
            state.focus,
            state.origin_location_id,
            state.target_location_id,
            state.unread_alerts.len(),
            live_frame.runner.phase,
            live_frame.runner.paused,
            live_frame.runner.tick_interval_ms
        ),
        state.output_lines.join("\n"),
    ];
    let bottom = Paragraph::new(bottom_text.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("Controls"))
        .wrap(Wrap { trim: true });
    frame.render_widget(bottom, layout[2]);

    if let Some(modal) = &state.modal {
        draw_modal(frame, modal);
    }
}

fn draw_location_list(
    frame: &mut ratatui::Frame<'_>,
    live_frame: &PlayerFrameResponse,
    state: &TuiState,
    area: Rect,
) {
    let items = live_frame
        .view
        .locations
        .iter()
        .map(|location| {
            let mut prefix = String::new();
            if state.origin_location_id == Some(location.location_id) {
                prefix.push('O');
            }
            if state.target_location_id == Some(location.location_id) {
                prefix.push('T');
            }
            if prefix.is_empty() {
                prefix.push(' ');
            }
            ListItem::new(format!(
                "[{}] #{:>2} {:<18} {:?}",
                prefix, location.location_id, location.name, location.visibility
            ))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if state.focus == FocusPane::Map {
                    "Locations *"
                } else {
                    "Locations"
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    list_state.select(if live_frame.view.locations.is_empty() {
        None
    } else {
        Some(state.selected_location_index)
    });
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_details(
    frame: &mut ratatui::Frame<'_>,
    live_frame: &PlayerFrameResponse,
    state: &TuiState,
    area: Rect,
) {
    let details = live_frame
        .view
        .locations
        .get(state.selected_location_index)
        .map(render_location)
        .unwrap_or_else(|| "No location selected".to_owned());
    let hints = action_hints(live_frame, state).join("\n");
    let content = if hints.is_empty() {
        details
    } else {
        format!("{details}\n\nAction hints:\n{hints}")
    };
    let widget = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(
            if state.focus == FocusPane::Overview {
                "Details *"
            } else {
                "Details"
            },
        ))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn draw_events(frame: &mut ratatui::Frame<'_>, state: &TuiState, area: Rect) {
    let mut lines = state
        .recent_events
        .iter()
        .rev()
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    if !state.unread_alerts.is_empty() {
        lines.push(String::new());
        lines.push("Unread alerts:".to_owned());
        lines.extend(
            state
                .unread_alerts
                .iter()
                .rev()
                .take(4)
                .map(|alert| format!("  {}", alert.title)),
        );
    }
    let widget =
        Paragraph::new(lines.join("\n"))
            .block(Block::default().borders(Borders::ALL).title(
                if state.focus == FocusPane::Events {
                    "Events *"
                } else {
                    "Events"
                },
            ))
            .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn draw_modal(frame: &mut ratatui::Frame<'_>, modal: &ModalState) {
    let area = centered_rect(70, 30, frame.area());
    frame.render_widget(Clear, area);

    let (title, body) = match modal {
        ModalState::Help => (
            "Help",
            "q quit\nR ready toggle\nSpace pause/resume sandbox\n1/2/3 speed presets\nUse : for typed commands like `status`, `map`, `events --from-tick 10`, `survey --origin 1 --destination 17`, `budget --upkeep 0 --research 16 --training 0 --agents 0`, or `research --branch models --target-level 1`.\nHotkeys: s survey, p pacify, c claim, a assault, x strike, g research.",
        ),
        ModalState::CommandLine { input } => ("Command", input.as_str()),
        ModalState::Build { input } => ("Build", input.as_str()),
        ModalState::Repair { input } => ("Repair", input.as_str()),
        ModalState::Budget { input } => ("Budget", input.as_str()),
        ModalState::Research { input } => ("Research", input.as_str()),
        ModalState::Train { input } => ("Train", input.as_str()),
    };
    let widget = Paragraph::new(body)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn centered_rect(horizontal_percent: u16, vertical_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - vertical_percent) / 2),
            Constraint::Percentage(vertical_percent),
            Constraint::Percentage((100 - vertical_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - horizontal_percent) / 2),
            Constraint::Percentage(horizontal_percent),
            Constraint::Percentage((100 - horizontal_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn action_hints(frame: &PlayerFrameResponse, state: &TuiState) -> Vec<String> {
    let mut hints = Vec::new();
    if let Some(target_id) = state.target_location_id
        && let Some(target) = frame
            .view
            .locations
            .iter()
            .find(|location| location.location_id == target_id)
    {
        if target.visibility == LocationVisibility::Observed && target.controller.is_none() {
            hints.push(
                "Observed neutral worlds can often be claimed after survey/pacify.".to_owned(),
            );
        }
        if target.controller.is_some() && target.controller != Some(frame.view.player_id) {
            hints.push(
                "Surveyed enemy worlds can be hit with `x` for a strategic strike.".to_owned(),
            );
        }
        if target.hostile_remnant_present == Some(true) {
            hints.push("Target has a hostile remnant. Pacify before claiming.".to_owned());
        }
        if target
            .contesting_players
            .as_ref()
            .is_some_and(|players| !players.is_empty())
        {
            hints.push("Target is contested. Expect combat-related follow-up events.".to_owned());
        }
        if target.territory == starforge_core::TerritoryState::Contested
            && target.controller != Some(frame.view.player_id)
            && (target.economy.is_none() || target.infrastructure_projects.is_none())
        {
            hints.push(
                "Enemy contested worlds now hide economy and project intel until takeover resolves."
                    .to_owned(),
            );
        }
        if target
            .pacification_ticks_remaining
            .is_some_and(|ticks_remaining| ticks_remaining > 0)
        {
            hints.push(
                "Pacification is active here; world output is reduced until the timer clears."
                    .to_owned(),
            );
        }
        if target.infrastructure.as_ref().is_some_and(|infra| {
            infra
                .iter()
                .any(|item| item.condition != starforge_core::InfrastructureCondition::Operational)
        }) {
            hints.push("Selected world has damaged infrastructure and may need repair.".to_owned());
        }
    }
    if state.origin_location_id.is_none() {
        hints.push("Set an origin with `o` before using expedition hotkeys.".to_owned());
    }
    if frame.view.research.active_project.is_none() {
        hints.push(
            "Reserve research throughput with `u`, then press `g` to start a project.".to_owned(),
        );
    }
    match &frame.view.collapse {
        starforge_core::CommandCollapseState::Stable => {}
        starforge_core::CommandCollapseState::Collapsing { ticks_remaining } => {
            hints.push(format!(
                "Command collapse active. Restore owned presence within {} ticks or be defeated.",
                ticks_remaining
            ))
        }
        starforge_core::CommandCollapseState::Defeated => {
            hints.push("This player is defeated and can no longer issue commands.".to_owned());
        }
    }
    if frame.runner.phase == SessionPhase::Lobby {
        hints.push("Match is in lobby. Press `R` to toggle ready.".to_owned());
    }
    if frame.runner.mode == SessionMode::Sandbox {
        hints.push("Sandbox controls: Space pauses, 1/2/3 change speed.".to_owned());
    }
    hints
}

fn live_status_lines(
    frame: &PlayerFrameResponse,
    player_id: PlayerId,
    state: &TuiState,
) -> Vec<String> {
    let owned_worlds = frame
        .view
        .locations
        .iter()
        .filter(|location| location.visibility == LocationVisibility::Owned)
        .count();
    let training = match &frame.view.training {
        Some(training) => {
            let site_suffix = training
                .ascension_site_location_id
                .map(|location_id| format!(" site=#{location_id}"))
                .unwrap_or_default();
            format!(
                "Training tier {} {}/{} need {}{}",
                training.target_tier,
                training.progress_ticks,
                training.required_ticks,
                training.required_training_throughput,
                site_suffix
            )
        }
        None => "Training idle".to_owned(),
    };

    vec![
        format!(
            "Live session {}  Tick {}  {:?}  {:?} paused={} {}ms",
            frame.session_id.0,
            frame.summary.current_tick.0,
            frame.summary.victory,
            frame.runner.phase,
            frame.runner.paused,
            frame.runner.tick_interval_ms
        ),
        format!(
            "P{} tier {} collapse={} owned={} alerts={} origin={:?} target={:?}",
            player_id.0,
            frame.view.model_tier,
            render_collapse(&frame.view.collapse),
            owned_worlds,
            state.unread_alerts.len(),
            state.origin_location_id,
            state.target_location_id
        ),
        format!(
            "Throughput avail={} upkeep={} research={} training={} agents={}  Stockpiles {}",
            frame.view.throughput.available,
            frame.view.throughput.reserved_for_model_upkeep,
            frame.view.throughput.reserved_for_research,
            frame.view.throughput.reserved_for_training,
            frame.view.throughput.reserved_for_agents,
            format_stockpiles(&frame.view.economy.connected_stockpiles)
        ),
        format!(
            "Research i/m/w/r={}/{}/{}/{}  Project={}",
            frame.view.research.industry_level,
            frame.view.research.models_level,
            frame.view.research.warfare_level,
            frame.view.research.resilience_level,
            frame
                .view
                .research
                .active_project
                .as_ref()
                .map(|project| format!(
                    "{} {} {}/{}",
                    render_research_branch(project.branch),
                    project.target_level,
                    project.progress_ticks,
                    project.required_ticks
                ))
                .unwrap_or_else(|| "none".to_owned())
        ),
        training,
    ]
}

struct ApiClient {
    base_url: String,
    client: Client,
}

impl ApiClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            client: Client::new(),
        }
    }

    async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> Result<SessionInfoResponse, DynError> {
        self.send_json(Method::POST, "/live/sessions", Some(&request))
            .await
    }

    async fn join_session(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
    ) -> Result<SessionInfoResponse, DynError> {
        self.send_json(
            Method::POST,
            &format!("/live/sessions/{}/join", session_id.0),
            Some(&JoinSessionRequest { player_id }),
        )
        .await
    }

    async fn set_ready(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
        ready: bool,
    ) -> Result<SessionInfoResponse, DynError> {
        self.send_json(
            Method::POST,
            &format!("/live/sessions/{}/ready", session_id.0),
            Some(&ReadySessionRequest { player_id, ready }),
        )
        .await
    }

    async fn issue_command(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
        command: CommandKind,
    ) -> Result<SessionInfoResponse, DynError> {
        self.send_json(
            Method::POST,
            &format!("/live/sessions/{}/commands", session_id.0),
            Some(&PlayerCommandRequest { player_id, command }),
        )
        .await
    }

    async fn run_session(&self, session_id: SessionId) -> Result<SessionInfoResponse, DynError> {
        self.send_json::<(), _>(
            Method::POST,
            &format!("/live/sessions/{}/run", session_id.0),
            None,
        )
        .await
    }

    async fn pause_session(&self, session_id: SessionId) -> Result<SessionInfoResponse, DynError> {
        self.send_json::<(), _>(
            Method::POST,
            &format!("/live/sessions/{}/pause", session_id.0),
            None,
        )
        .await
    }

    async fn set_speed(
        &self,
        session_id: SessionId,
        tick_interval_ms: u64,
    ) -> Result<SessionInfoResponse, DynError> {
        self.send_json(
            Method::POST,
            &format!("/live/sessions/{}/speed", session_id.0),
            Some(&RunnerSpeedRequest { tick_interval_ms }),
        )
        .await
    }

    async fn frame(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
        from_event_index: usize,
    ) -> Result<PlayerFrameResponse, DynError> {
        let response = self
            .client
            .request(
                Method::GET,
                format!("{}/live/sessions/{}/frame", self.base_url, session_id.0),
            )
            .query(&[
                ("player_id", player_id.0.to_string()),
                ("from_event_index", from_event_index.to_string()),
            ])
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            let status = response.status();
            let body = response.text().await?;
            Err(io::Error::other(format!("api request failed: {status} {body}")).into())
        }
    }

    async fn send_json<Req, Res>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Req>,
    ) -> Result<Res, DynError>
    where
        Req: serde::Serialize + ?Sized,
        Res: serde::de::DeserializeOwned,
    {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await?;
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            let status = response.status();
            let body = response.text().await?;
            Err(io::Error::other(format!("api request failed: {status} {body}")).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use starforge_api::PlayerAlertKind;

    use super::{FocusPane, ModalState, TuiState, default_research_command, report_ui_error};

    #[test]
    fn reducer_sets_origin_from_selection() {
        let mut state = TuiState {
            selected_location_index: 0,
            origin_location_id: None,
            target_location_id: None,
            focus: FocusPane::Overview,
            modal: None,
            unread_alerts: Vec::new(),
            recent_events: Vec::new(),
            output_lines: Vec::new(),
        };
        let frame = crate::live_test_frame();
        state.set_origin_from_selection(&frame, starforge_core::PlayerId::new(1));
        assert_eq!(state.origin_location_id, Some(1));
    }

    #[test]
    fn reducer_sets_target_from_selection() {
        let mut state = TuiState {
            selected_location_index: 1,
            origin_location_id: None,
            target_location_id: None,
            focus: FocusPane::Overview,
            modal: None,
            unread_alerts: Vec::new(),
            recent_events: Vec::new(),
            output_lines: Vec::new(),
        };
        let frame = crate::live_test_frame();
        state.set_target_from_selection(&frame);
        assert_eq!(state.target_location_id, Some(2));
    }

    #[test]
    fn reducer_opens_modal() {
        let mut state = TuiState {
            selected_location_index: 0,
            origin_location_id: None,
            target_location_id: None,
            focus: FocusPane::Overview,
            modal: None,
            unread_alerts: Vec::new(),
            recent_events: Vec::new(),
            output_lines: Vec::new(),
        };
        state.open_modal(ModalState::Help);
        assert_eq!(state.modal, Some(ModalState::Help));
    }

    #[test]
    fn reducer_acknowledges_alerts() {
        let mut state = TuiState {
            selected_location_index: 0,
            origin_location_id: None,
            target_location_id: None,
            focus: FocusPane::Overview,
            modal: None,
            unread_alerts: vec![starforge_api::PlayerAlert {
                kind: PlayerAlertKind::Survey,
                title: "location #2 surveyed".to_owned(),
                tick_id: starforge_core::TickId::new(10),
                location_id: Some(2),
            }],
            recent_events: Vec::new(),
            output_lines: Vec::new(),
        };
        state.acknowledge_alerts();
        assert!(state.unread_alerts.is_empty());
    }

    #[test]
    fn default_research_modal_prefers_lowest_level_branch() {
        let frame = crate::live_test_frame();
        assert_eq!(
            default_research_command(&frame),
            "research --branch industry --target-level 1"
        );
    }

    #[test]
    fn budget_modal_tracks_research_field() {
        let frame = crate::live_test_frame();
        let input = format!(
            "budget --upkeep {} --research {} --training {} --agents {}",
            frame.view.throughput.reserved_for_model_upkeep,
            frame.view.throughput.reserved_for_research,
            frame.view.throughput.reserved_for_training,
            frame.view.throughput.reserved_for_agents
        );
        assert_eq!(
            input,
            "budget --upkeep 0 --research 0 --training 0 --agents 0"
        );
    }

    #[test]
    fn ui_errors_are_written_to_output_instead_of_bubbling() {
        let mut state = TuiState {
            selected_location_index: 0,
            origin_location_id: None,
            target_location_id: None,
            focus: FocusPane::Overview,
            modal: None,
            unread_alerts: Vec::new(),
            recent_events: Vec::new(),
            output_lines: Vec::new(),
        };

        report_ui_error(
            &mut state,
            &io::Error::other("api request failed: 409 Conflict"),
        );

        assert_eq!(
            state.output_lines,
            vec!["Error: api request failed: 409 Conflict".to_owned()]
        );
    }
}
