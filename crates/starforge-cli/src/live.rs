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
use starforge_core::{CommandKind, LocationVisibility, PlayerId, RelayStatus, SessionId};
use tokio::{sync::mpsc, time::sleep};

use crate::{
    cli::{PlayerEventsArgs, PlayerScopedCommand, parse_player_scoped_command},
    render::{
        format_stockpiles, render_collapse, render_event, render_location, render_map_lines,
        render_status_lines,
    },
    tui_actions::{
        ActionFormState, ActionGroup, ActionId, FormSubmit, PaneFocus, action_by_id,
        action_form_for_selected, action_index, default_selected_action_id, derive_actions,
        event_history_custom_form, form_adjust_left, form_adjust_right, form_back, form_lines,
        form_next, form_previous, form_title, rebuild_form, selected_form_message,
    },
};

type DynError = Box<dyn Error>;
pub(crate) const TUI_SANDBOX_DEFAULT_TICK_INTERVAL_MS: u64 = 5_000;
const TUI_SPEED_SHORTCUTS_MS: [u64; 3] = [TUI_SANDBOX_DEFAULT_TICK_INTERVAL_MS, 2_500, 1_000];

pub(crate) async fn create_and_play(
    api_url: String,
    player_id: PlayerId,
    mode: SessionMode,
) -> Result<(), DynError> {
    let client = ApiClient::new(api_url);
    let tick_interval_override = default_tick_interval_override(mode.clone());
    let response = client
        .create_session(CreateSessionRequest {
            mode,
            claimed_player_id: Some(player_id),
        })
        .await?;
    if let Some(tick_interval_ms) = tick_interval_override {
        client
            .set_speed(response.session_id, tick_interval_ms)
            .await?;
    }
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
                match handle_event(&event, &mut state, &frame) {
                    Ok(Some(UiCommand::Quit)) => break,
                    Ok(Some(command)) => {
                        apply_ui_command(client, session_id, player_id, &mut state, &mut frame, command).await;
                    }
                    Ok(None) => {}
                    Err(error) => state.push_output(format!("Action failed: {error}")),
                }
            }
            _ = sleep(Duration::from_millis(100)) => {
                match client.frame(session_id, player_id, frame.next_event_index).await {
                    Ok(updated) => {
                        state.apply_frame(&updated);
                        frame = updated;
                    }
                    Err(error) => state.push_output(format!("Action failed: {error}")),
                }
            }
        }
    }

    Ok(())
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
pub(crate) enum ModalState {
    LegacyCommand { input: String },
    ActionForm(ActionFormState),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiState {
    pub(crate) selected_location_index: usize,
    pub(crate) selected_action_id: Option<ActionId>,
    pub(crate) focus: PaneFocus,
    pub(crate) modal: Option<ModalState>,
    pub(crate) unread_alerts: Vec<PlayerAlert>,
    recent_events: Vec<String>,
    output_lines: Vec<String>,
}

impl TuiState {
    pub(crate) fn from_frame(frame: &PlayerFrameResponse) -> Self {
        let mut state = Self {
            selected_location_index: 0,
            selected_action_id: None,
            focus: PaneFocus::Locations,
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
            self.selected_action_id = None;
        } else if self.selected_location_index >= frame.view.locations.len() {
            self.selected_location_index = frame.view.locations.len().saturating_sub(1);
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
            if let Some(location_id) = frame.alerts.iter().find_map(|alert| alert.location_id)
                && let Some(index) = frame
                    .view
                    .locations
                    .iter()
                    .position(|location| location.location_id == location_id)
            {
                self.selected_location_index = index;
            }
        }

        self.reconcile_selected_action(frame);
    }

    pub(crate) fn select_next_location(&mut self, frame: &PlayerFrameResponse) {
        if frame.view.locations.is_empty() {
            self.selected_location_index = 0;
            return;
        }

        self.selected_location_index =
            (self.selected_location_index + 1) % frame.view.locations.len();
        self.reconcile_selected_action(frame);
    }

    pub(crate) fn select_previous_location(&mut self, frame: &PlayerFrameResponse) {
        if frame.view.locations.is_empty() {
            self.selected_location_index = 0;
            return;
        }

        self.selected_location_index = (self.selected_location_index + frame.view.locations.len()
            - 1)
            % frame.view.locations.len();
        self.reconcile_selected_action(frame);
    }

    pub(crate) fn select_next_action(&mut self, frame: &PlayerFrameResponse) {
        let actions = derive_actions(frame, self.selected_location_index);
        if actions.is_empty() {
            self.selected_action_id = None;
            return;
        }

        let next_index = self
            .selected_action_id
            .and_then(|action_id| action_index(&actions, action_id))
            .map(|index| (index + 1) % actions.len())
            .unwrap_or(0);
        self.selected_action_id = Some(actions[next_index].id);
    }

    pub(crate) fn select_previous_action(&mut self, frame: &PlayerFrameResponse) {
        let actions = derive_actions(frame, self.selected_location_index);
        if actions.is_empty() {
            self.selected_action_id = None;
            return;
        }

        let previous_index = self
            .selected_action_id
            .and_then(|action_id| action_index(&actions, action_id))
            .map(|index| (index + actions.len() - 1) % actions.len())
            .unwrap_or(0);
        self.selected_action_id = Some(actions[previous_index].id);
    }

    pub(crate) fn acknowledge_alerts(&mut self) {
        self.unread_alerts.clear();
    }

    pub(crate) fn push_output(&mut self, line: String) {
        self.output_lines.push(line);
        if self.output_lines.len() > 12 {
            let drop_count = self.output_lines.len() - 12;
            self.output_lines.drain(..drop_count);
        }
    }

    fn reconcile_selected_action(&mut self, frame: &PlayerFrameResponse) {
        let actions = derive_actions(frame, self.selected_location_index);
        self.selected_action_id = match self.selected_action_id {
            Some(action_id) if action_by_id(&actions, action_id).is_some() => Some(action_id),
            _ => default_selected_action_id(&actions),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UiCommand {
    Quit,
    ExecuteParsed(PlayerScopedCommand),
    RefreshAllEvents(u64),
    ToggleReady,
    TogglePause,
    SetSpeed(u64),
    ToggleRelay,
    SubmitForm(ActionFormState, FormSubmit),
}

fn handle_event(
    event: &Event,
    state: &mut TuiState,
    frame: &PlayerFrameResponse,
) -> Result<Option<UiCommand>, DynError> {
    let Event::Key(key) = event else {
        return Ok(None);
    };

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(Some(UiCommand::Quit));
    }

    if let Some(modal) = state.modal.take() {
        return handle_modal_key(key, state, frame, modal);
    }

    match key.code {
        KeyCode::Char('q') => Ok(Some(UiCommand::Quit)),
        KeyCode::Tab => {
            state.focus = match state.focus {
                PaneFocus::Locations => PaneFocus::Actions,
                PaneFocus::Actions => PaneFocus::Locations,
            };
            Ok(None)
        }
        KeyCode::BackTab => {
            state.focus = match state.focus {
                PaneFocus::Locations => PaneFocus::Actions,
                PaneFocus::Actions => PaneFocus::Locations,
            };
            Ok(None)
        }
        KeyCode::Up => {
            match state.focus {
                PaneFocus::Locations => state.select_previous_location(frame),
                PaneFocus::Actions => state.select_previous_action(frame),
            }
            Ok(None)
        }
        KeyCode::Down => {
            match state.focus {
                PaneFocus::Locations => state.select_next_location(frame),
                PaneFocus::Actions => state.select_next_action(frame),
            }
            Ok(None)
        }
        KeyCode::Enter => match state.focus {
            PaneFocus::Locations => {
                state.focus = PaneFocus::Actions;
                Ok(None)
            }
            PaneFocus::Actions => activate_selected_action(state, frame),
        },
        KeyCode::Char('s') => activate_action_by_id(ActionId::Survey, state, frame),
        KeyCode::Char('p') => activate_action_by_id(ActionId::Pacify, state, frame),
        KeyCode::Char('c') => activate_action_by_id(ActionId::Claim, state, frame),
        KeyCode::Char('a') => activate_action_by_id(ActionId::Assault, state, frame),
        KeyCode::Char('x') => activate_action_by_id(ActionId::Strike, state, frame),
        KeyCode::Char('b') => activate_action_by_id(ActionId::Build, state, frame),
        KeyCode::Char('r') => activate_action_by_id(ActionId::Repair, state, frame),
        KeyCode::Char('u') => activate_action_by_id(ActionId::Budget, state, frame),
        KeyCode::Char('g') => activate_action_by_id(ActionId::Research, state, frame),
        KeyCode::Char('t') => activate_action_by_id(ActionId::Training, state, frame),
        KeyCode::Char('l') => activate_action_by_id(ActionId::Relay, state, frame),
        KeyCode::Char(':') => {
            state.modal = Some(ModalState::LegacyCommand {
                input: String::new(),
            });
            Ok(None)
        }
        KeyCode::Char('?') => {
            state.modal = Some(ModalState::Help);
            Ok(None)
        }
        KeyCode::Esc => {
            state.acknowledge_alerts();
            Ok(None)
        }
        KeyCode::Char('R') => Ok(Some(UiCommand::ToggleReady)),
        KeyCode::Char(' ') => Ok(Some(UiCommand::TogglePause)),
        KeyCode::Char('1') => Ok(Some(UiCommand::SetSpeed(TUI_SPEED_SHORTCUTS_MS[0]))),
        KeyCode::Char('2') => Ok(Some(UiCommand::SetSpeed(TUI_SPEED_SHORTCUTS_MS[1]))),
        KeyCode::Char('3') => Ok(Some(UiCommand::SetSpeed(TUI_SPEED_SHORTCUTS_MS[2]))),
        _ => Ok(None),
    }
}

fn default_tick_interval_override(mode: SessionMode) -> Option<u64> {
    (mode == SessionMode::Sandbox).then_some(TUI_SANDBOX_DEFAULT_TICK_INTERVAL_MS)
}

fn handle_modal_key(
    key: &KeyEvent,
    state: &mut TuiState,
    frame: &PlayerFrameResponse,
    modal: ModalState,
) -> Result<Option<UiCommand>, DynError> {
    match modal {
        ModalState::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => Ok(None),
            _ => {
                state.modal = Some(ModalState::Help);
                Ok(None)
            }
        },
        ModalState::LegacyCommand { mut input } => match key.code {
            KeyCode::Esc => Ok(None),
            KeyCode::Backspace => {
                input.pop();
                state.modal = Some(ModalState::LegacyCommand { input });
                Ok(None)
            }
            KeyCode::Enter => {
                let command_text = input.trim().to_owned();
                if command_text.is_empty() {
                    return Ok(None);
                }
                match parse_player_scoped_command(&command_text) {
                    Ok(PlayerScopedCommand::Events(args)) => {
                        Ok(Some(UiCommand::RefreshAllEvents(args.from_tick)))
                    }
                    Ok(parsed) => Ok(Some(UiCommand::ExecuteParsed(parsed))),
                    Err(error) => {
                        state.push_output(format!("Action failed: {error}"));
                        state.modal = Some(ModalState::LegacyCommand { input });
                        Ok(None)
                    }
                }
            }
            KeyCode::Char(character) => {
                input.push(character);
                state.modal = Some(ModalState::LegacyCommand { input });
                Ok(None)
            }
            _ => {
                state.modal = Some(ModalState::LegacyCommand { input });
                Ok(None)
            }
        },
        ModalState::ActionForm(mut form) => match key.code {
            KeyCode::Esc => {
                state.modal = form_back(&form).map(ModalState::ActionForm);
                Ok(None)
            }
            KeyCode::Up => {
                form_previous(&mut form);
                state.modal = Some(ModalState::ActionForm(form));
                Ok(None)
            }
            KeyCode::Down => {
                form_next(&mut form);
                state.modal = Some(ModalState::ActionForm(form));
                Ok(None)
            }
            KeyCode::Left => {
                form_adjust_left(&mut form);
                state.modal = Some(ModalState::ActionForm(form));
                Ok(None)
            }
            KeyCode::Right => {
                form_adjust_right(&mut form);
                state.modal = Some(ModalState::ActionForm(form));
                Ok(None)
            }
            KeyCode::Enter => {
                if let Some(rebuilt) = rebuild_form(&form, frame) {
                    form = rebuilt;
                }
                match crate::tui_actions::submit_form(&form, frame) {
                    Ok(FormSubmit::OpenCustomEventHistory(from_tick)) => {
                        state.modal =
                            Some(ModalState::ActionForm(event_history_custom_form(from_tick)));
                        Ok(None)
                    }
                    Ok(submit) => {
                        state.modal = Some(ModalState::ActionForm(form.clone()));
                        Ok(Some(UiCommand::SubmitForm(form, submit)))
                    }
                    Err(reason) => {
                        state.push_output(format!("Action failed: {reason}"));
                        state.modal = Some(ModalState::ActionForm(form));
                        Ok(None)
                    }
                }
            }
            _ => {
                state.modal = Some(ModalState::ActionForm(form));
                Ok(None)
            }
        },
    }
}

fn activate_selected_action(
    state: &mut TuiState,
    frame: &PlayerFrameResponse,
) -> Result<Option<UiCommand>, DynError> {
    if let Some(action_id) = state.selected_action_id {
        activate_action_by_id(action_id, state, frame)
    } else {
        Ok(None)
    }
}

fn activate_action_by_id(
    action_id: ActionId,
    state: &mut TuiState,
    frame: &PlayerFrameResponse,
) -> Result<Option<UiCommand>, DynError> {
    let actions = derive_actions(frame, state.selected_location_index);
    let Some(action) = action_by_id(&actions, action_id) else {
        return Ok(None);
    };

    state.selected_action_id = Some(action.id);
    if let Some(reason) = action.availability.reason() {
        state.push_output(format!("Action failed: {reason}"));
        return Ok(None);
    }

    if action.opens_form {
        if let Some(form) =
            action_form_for_selected(action.id, frame, state.selected_location_index)
        {
            state.modal = Some(ModalState::ActionForm(form));
        }
        return Ok(None);
    }

    match action.id {
        ActionId::Status => Ok(Some(UiCommand::ExecuteParsed(PlayerScopedCommand::Status))),
        ActionId::Map => Ok(Some(UiCommand::ExecuteParsed(PlayerScopedCommand::Map))),
        ActionId::Ready => Ok(Some(UiCommand::ToggleReady)),
        ActionId::PauseResume => Ok(Some(UiCommand::TogglePause)),
        ActionId::Relay => Ok(Some(UiCommand::ToggleRelay)),
        ActionId::Events
        | ActionId::Budget
        | ActionId::Research
        | ActionId::Training
        | ActionId::Speed
        | ActionId::Survey
        | ActionId::Pacify
        | ActionId::Claim
        | ActionId::Assault
        | ActionId::Strike
        | ActionId::Build
        | ActionId::Repair => Ok(None),
    }
}

async fn apply_ui_command(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
    command: UiCommand,
) {
    match command {
        UiCommand::Quit => {}
        UiCommand::ExecuteParsed(parsed) => {
            if let Err(error) =
                execute_player_scoped_command(client, session_id, player_id, state, frame, parsed)
                    .await
            {
                state.push_output(format!("Action failed: {error}"));
                refresh_after_failure(client, session_id, player_id, state, frame).await;
            }
        }
        UiCommand::RefreshAllEvents(from_tick) => {
            if let Err(error) =
                refresh_all_events(client, session_id, player_id, state, frame, from_tick).await
            {
                state.push_output(format!("Action failed: {error}"));
                refresh_after_failure(client, session_id, player_id, state, frame).await;
            }
        }
        UiCommand::ToggleReady => {
            if let Err(error) = toggle_ready(client, session_id, player_id, state, frame).await {
                state.push_output(format!("Action failed: {error}"));
                refresh_after_failure(client, session_id, player_id, state, frame).await;
            }
        }
        UiCommand::TogglePause => {
            if let Err(error) = toggle_pause(client, session_id, state, frame).await {
                state.push_output(format!("Action failed: {error}"));
                refresh_after_failure(client, session_id, player_id, state, frame).await;
            }
        }
        UiCommand::SetSpeed(speed) => {
            if let Err(error) = set_speed(client, session_id, state, frame, speed).await {
                state.push_output(format!("Action failed: {error}"));
                refresh_after_failure(client, session_id, player_id, state, frame).await;
            }
        }
        UiCommand::ToggleRelay => {
            if let Err(error) = toggle_relay(client, session_id, player_id, state, frame).await {
                state.push_output(format!("Action failed: {error}"));
                refresh_after_failure(client, session_id, player_id, state, frame).await;
            }
        }
        UiCommand::SubmitForm(form, submit) => {
            let result = match submit {
                FormSubmit::Command(command) => {
                    execute_player_scoped_command(
                        client, session_id, player_id, state, frame, command,
                    )
                    .await
                }
                FormSubmit::RefreshEvents(from_tick) => {
                    refresh_all_events(client, session_id, player_id, state, frame, from_tick).await
                }
                FormSubmit::OpenCustomEventHistory(_) => Ok(()),
                FormSubmit::SetSpeed(speed) => {
                    set_speed(client, session_id, state, frame, speed).await
                }
            };

            match result {
                Ok(()) => state.modal = None,
                Err(error) => {
                    state.push_output(format!("Action failed: {error}"));
                    state.modal = rebuild_form(&form, frame).map(ModalState::ActionForm);
                    if state.modal.is_none() {
                        state.modal = Some(ModalState::ActionForm(form));
                    }
                    refresh_after_failure(client, session_id, player_id, state, frame).await;
                }
            }
        }
    }
}

async fn refresh_all_events(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
    from_tick: u64,
) -> Result<(), DynError> {
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
    Ok(())
}

async fn toggle_ready(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
) -> Result<(), DynError> {
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
    Ok(())
}

async fn toggle_pause(
    client: &ApiClient,
    session_id: SessionId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
) -> Result<(), DynError> {
    if frame.runner.paused {
        client.run_session(session_id).await?;
        state.push_output("Sandbox session resumed".to_owned());
    } else {
        client.pause_session(session_id).await?;
        state.push_output("Sandbox session paused".to_owned());
    }
    let updated = client
        .frame(session_id, frame.view.player_id, frame.next_event_index)
        .await?;
    state.apply_frame(&updated);
    *frame = updated;
    Ok(())
}

async fn set_speed(
    client: &ApiClient,
    session_id: SessionId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
    speed: u64,
) -> Result<(), DynError> {
    client.set_speed(session_id, speed).await?;
    state.push_output(format!("Tick speed set to {speed} ms"));
    let updated = client
        .frame(session_id, frame.view.player_id, frame.next_event_index)
        .await?;
    state.apply_frame(&updated);
    *frame = updated;
    Ok(())
}

async fn toggle_relay(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
) -> Result<(), DynError> {
    let Some(location) = frame.view.locations.get(state.selected_location_index) else {
        return Ok(());
    };
    let Some(relay_status) = location.relay_status.clone() else {
        return Ok(());
    };
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
    .await
}

async fn refresh_after_failure(
    client: &ApiClient,
    session_id: SessionId,
    player_id: PlayerId,
    state: &mut TuiState,
    frame: &mut PlayerFrameResponse,
) {
    if let Ok(updated) = client
        .frame(session_id, player_id, frame.next_event_index)
        .await
    {
        state.apply_frame(&updated);
        *frame = updated;
    }
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
        PlayerScopedCommand::Events(PlayerEventsArgs { from_tick }) => {
            refresh_all_events(client, session_id, player_id, state, frame, *from_tick).await?;
        }
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
            Constraint::Length(8),
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

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(middle[2]);

    draw_location_list(frame, live_frame, state, middle[0]);
    draw_details(frame, live_frame, state, middle[1]);
    draw_actions(frame, live_frame, state, right[0]);
    draw_events(frame, state, right[1]);

    let bottom_text = [
        current_message(live_frame, state),
        "Tab switch pane  Up/Down move  Enter select  Left/Right adjust  Esc back  ? help  : advanced  q quit".to_owned(),
        format!(
            "Pane={:?}  Alerts={}  Runner={:?} paused={} speed={}ms",
            state.focus,
            state.unread_alerts.len(),
            live_frame.runner.phase,
            live_frame.runner.paused,
            live_frame.runner.tick_interval_ms
        ),
        state.output_lines.join("\n"),
    ];
    let bottom = Paragraph::new(bottom_text.join("\n"))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Controls / Output"),
        )
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
            ListItem::new(format!(
                "#{:>2} {:<18} {:?}",
                location.location_id, location.name, location.visibility
            ))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(
            if state.focus == PaneFocus::Locations {
                "Locations *"
            } else {
                "Locations"
            },
        ))
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
    let hints = action_hints(live_frame, state.selected_location_index).join("\n");
    let content = if hints.is_empty() {
        details
    } else {
        format!("{details}\n\nAction hints:\n{hints}")
    };
    let widget = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn draw_actions(
    frame: &mut ratatui::Frame<'_>,
    live_frame: &PlayerFrameResponse,
    state: &TuiState,
    area: Rect,
) {
    let actions = derive_actions(live_frame, state.selected_location_index);
    let items = actions
        .iter()
        .map(|action| {
            let status = if action.availability.is_enabled() {
                ' '
            } else {
                'x'
            };
            let group = match action.group {
                ActionGroup::Location => "World",
                ActionGroup::Session => "Session",
            };
            ListItem::new(format!("[{status}] {group}: {}", action.label))
        })
        .collect::<Vec<_>>();

    let list =
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(
                if state.focus == PaneFocus::Actions {
                    "Actions *"
                } else {
                    "Actions"
                },
            ))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    list_state.select(
        state
            .selected_action_id
            .and_then(|action_id| action_index(&actions, action_id)),
    );
    frame.render_stateful_widget(list, area, &mut list_state);
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
    let widget = Paragraph::new(lines.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("Events"))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn draw_modal(frame: &mut ratatui::Frame<'_>, modal: &ModalState) {
    let area = centered_rect(80, 45, frame.area());
    frame.render_widget(Clear, area);

    let (title, body) = match modal {
        ModalState::Help => (
            "Help".to_owned(),
            [
                "Tab switches between the Locations and Actions panes.".to_owned(),
                "Up/Down moves within the focused pane.".to_owned(),
                "Enter opens the selected action or confirms the current form.".to_owned(),
                "Left/Right adjusts numeric form fields.".to_owned(),
                "Esc closes the current form or clears unread alerts.".to_owned(),
                ": opens the advanced command line.".to_owned(),
                "Legacy shortcuts still work: s/p/c/a/x b/r/u/g/t l R Space 1/2/3 q".to_owned(),
            ]
            .join("\n"),
        ),
        ModalState::LegacyCommand { input } => ("Advanced Command".to_owned(), input.clone()),
        ModalState::ActionForm(form) => (form_title(form), form_lines(form).join("\n")),
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

fn current_message(frame: &PlayerFrameResponse, state: &TuiState) -> String {
    if let Some(ModalState::ActionForm(form)) = &state.modal
        && let Some(message) = selected_form_message(form)
    {
        return message;
    }

    let actions = derive_actions(frame, state.selected_location_index);
    state
        .selected_action_id
        .and_then(|action_id| action_by_id(&actions, action_id))
        .map(|action| action.summary())
        .unwrap_or_else(|| "Select a world, then choose an action.".to_owned())
}

fn action_hints(frame: &PlayerFrameResponse, selected_location_index: usize) -> Vec<String> {
    let mut hints = Vec::new();
    let Some(location) = frame.view.locations.get(selected_location_index) else {
        return hints;
    };

    if location.visibility == LocationVisibility::Observed && location.controller.is_none() {
        hints.push("Observed neutral worlds can usually be surveyed, then claimed.".to_owned());
    }
    if location.controller.is_some() && location.controller != Some(frame.view.player_id) {
        hints.push(
            "Enemy worlds usually need survey before assault or strategic strike.".to_owned(),
        );
    }
    if location.hostile_remnant_present == Some(true) {
        hints.push("Clear hostile remnants before claiming this world.".to_owned());
    }
    if location.territory == starforge_core::TerritoryState::Contested
        && location.controller != Some(frame.view.player_id)
        && (location.economy.is_none() || location.infrastructure_projects.is_none())
    {
        hints.push(
            "Enemy contested worlds hide economy and project details until takeover resolves."
                .to_owned(),
        );
    }
    if location
        .pacification_ticks_remaining
        .is_some_and(|ticks_remaining| ticks_remaining > 0)
    {
        hints.push("Pacification is active here; output is reduced until it clears.".to_owned());
    }
    if location.infrastructure.as_ref().is_some_and(|infra| {
        infra
            .iter()
            .any(|item| item.condition != starforge_core::InfrastructureCondition::Operational)
    }) {
        hints.push("This world has damaged infrastructure and may need repair.".to_owned());
    }
    if frame.view.research.active_project.is_none() {
        hints
            .push("Use Budget and Research in the Actions pane to start a new project.".to_owned());
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
        hints.push("Match is in the lobby. Select Mark Ready from the Actions pane.".to_owned());
    }
    if frame.runner.mode == SessionMode::Sandbox {
        hints.push("Sandbox runner controls are available in the Session actions list.".to_owned());
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
    let selected_location = frame
        .view
        .locations
        .get(state.selected_location_index)
        .map(|location| format!("#{} {}", location.location_id, location.name))
        .unwrap_or_else(|| "none".to_owned());

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
            "P{} tier {} collapse={} owned={} alerts={} selected={} pane={:?}",
            player_id.0,
            frame.view.model_tier,
            render_collapse(&frame.view.collapse),
            owned_worlds,
            state.unread_alerts.len(),
            selected_location,
            state.focus
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
                    "{:?} {} {}/{}",
                    project.branch,
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
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use starforge_api::PlayerAlertKind;

    use crate::tui_actions::{ActionFormState, ActionId, PaneFocus, action_form_for_selected};

    use super::{
        ModalState, TUI_SANDBOX_DEFAULT_TICK_INTERVAL_MS, TuiState, UiCommand,
        default_tick_interval_override, handle_event, handle_modal_key,
    };

    #[test]
    fn from_frame_selects_first_visible_action() {
        let mut frame = crate::live_test_frame();
        frame.alerts.clear();
        let state = TuiState::from_frame(&frame);
        assert_eq!(state.selected_action_id, Some(ActionId::Build));
    }

    #[test]
    fn apply_frame_retargets_alerted_location() {
        let mut state = TuiState::from_frame(&crate::live_test_frame());
        let frame = crate::live_test_frame();
        state.apply_frame(&frame);
        assert_eq!(state.selected_location_index, 1);
        assert_eq!(state.selected_action_id, Some(ActionId::Survey));
    }

    #[test]
    fn reducer_acknowledges_alerts() {
        let mut state = TuiState {
            selected_location_index: 0,
            selected_action_id: Some(ActionId::Build),
            focus: PaneFocus::Locations,
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
    fn invalid_budget_submit_keeps_action_form_open() {
        let frame = crate::live_test_frame();
        let mut state = TuiState::from_frame(&frame);
        let mut form = action_form_for_selected(ActionId::Budget, &frame, 0).expect("budget form");
        if let ActionFormState::Budget {
            upkeep,
            total_available,
            ..
        } = &mut form
        {
            *upkeep = *total_available + 1;
        } else {
            panic!("expected budget form");
        }

        let result = handle_modal_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut state,
            &frame,
            ModalState::ActionForm(form),
        )
        .expect("modal should handle invalid budget");

        assert!(result.is_none());
        assert!(matches!(state.modal, Some(ModalState::ActionForm(_))));
        assert!(
            state
                .output_lines
                .last()
                .expect("error output")
                .contains("Action failed")
        );
    }

    #[test]
    fn sandbox_create_defaults_to_five_second_ticks() {
        assert_eq!(
            default_tick_interval_override(starforge_api::SessionMode::Sandbox),
            Some(TUI_SANDBOX_DEFAULT_TICK_INTERVAL_MS)
        );
        assert_eq!(
            default_tick_interval_override(starforge_api::SessionMode::Competitive),
            None
        );
    }

    #[test]
    fn speed_shortcuts_use_slower_presets() {
        let frame = crate::live_test_frame();
        let mut state = TuiState::from_frame(&frame);

        let shortcut_one = handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)),
            &mut state,
            &frame,
        )
        .expect("shortcut should parse");
        assert_eq!(
            shortcut_one,
            Some(UiCommand::SetSpeed(TUI_SANDBOX_DEFAULT_TICK_INTERVAL_MS))
        );

        let shortcut_two = handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            &mut state,
            &frame,
        )
        .expect("shortcut should parse");
        assert_eq!(shortcut_two, Some(UiCommand::SetSpeed(2_500)));

        let shortcut_three = handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE)),
            &mut state,
            &frame,
        )
        .expect("shortcut should parse");
        assert_eq!(shortcut_three, Some(UiCommand::SetSpeed(1_000)));
    }
}
