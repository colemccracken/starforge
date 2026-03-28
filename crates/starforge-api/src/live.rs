use std::{
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use starforge_content::ContentError;
use starforge_core::{
    CommandKind, EventKind, GameSession, IndexedEventRecord, LocationConnection,
    LocationVisibility, PlayerId, PlayerStateView, SessionId, SnapshotError, TickId,
    ValidationError, VictoryState,
};
use tokio::{
    sync::{Mutex, Notify, RwLock},
    time::{Duration, sleep},
};

use crate::{ApiBootstrapError, ApiServerConfig, load_session_summary};

const DEFAULT_TICK_INTERVAL_MS: u64 = 250;
const ALLOWED_TICK_INTERVAL_MS: [u64; 6] = [125, 250, 500, 1_000, 2_500, 5_000];
const PERSISTED_SESSION_META_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Competitive,
    Sandbox,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Lobby,
    Running,
    Finished,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSeat {
    pub player_id: PlayerId,
    pub claimed: bool,
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerStatus {
    pub mode: SessionMode,
    pub phase: SessionPhase,
    pub tick_interval_ms: u64,
    pub pause_allowed: bool,
    pub speed_change_allowed: bool,
    pub paused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAlertKind {
    Arrival,
    Survey,
    Claim,
    Contest,
    Capture,
    Pacification,
    Repair,
    Strike,
    Research,
    Training,
    Ascension,
    Collapse,
    Defeat,
    Victory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerAlert {
    pub kind: PlayerAlertKind,
    pub title: String,
    pub tick_id: TickId,
    pub location_id: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownRouteView {
    pub from_location_id: u32,
    pub to_location_id: u32,
    pub travel_time_ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSessionSummary {
    pub scenario_name: String,
    pub current_tick: TickId,
    pub victory: VictoryState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfoResponse {
    pub session_id: SessionId,
    pub summary: LiveSessionSummary,
    pub seats: Vec<PlayerSeat>,
    pub runner: RunnerStatus,
    pub state_hash: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerFrameResponse {
    pub session_id: SessionId,
    pub summary: LiveSessionSummary,
    pub seats: Vec<PlayerSeat>,
    pub runner: RunnerStatus,
    pub state_hash: u64,
    pub next_event_index: usize,
    pub view: PlayerStateView,
    pub events: Vec<IndexedEventRecord>,
    pub alerts: Vec<PlayerAlert>,
    pub known_routes: Vec<KnownRouteView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub mode: SessionMode,
    pub claimed_player_id: Option<PlayerId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinSessionRequest {
    pub player_id: PlayerId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadySessionRequest {
    pub player_id: PlayerId,
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerCommandRequest {
    pub player_id: PlayerId,
    pub command: CommandKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerSpeedRequest {
    pub tick_interval_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PlayerFrameQuery {
    pub player_id: PlayerId,
    #[serde(default)]
    pub from_event_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedSessionMeta {
    version: u32,
    session_id: SessionId,
    mode: SessionMode,
    phase: SessionPhase,
    tick_interval_ms: u64,
    paused: bool,
    seats: Vec<PlayerSeat>,
}

impl PersistedSessionMeta {
    fn from_state(state: &LiveSessionState) -> Self {
        Self {
            version: PERSISTED_SESSION_META_VERSION,
            session_id: state.session.session_id(),
            mode: state.mode.clone(),
            phase: state.phase.clone(),
            tick_interval_ms: state.tick_interval_ms,
            paused: state.paused,
            seats: state.seats.clone(),
        }
    }
}

#[derive(Debug)]
pub enum LiveSessionError {
    NotFound(String),
    Conflict(String),
    Invalid(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Snapshot(SnapshotError),
    Content(ContentError),
    Validation(ValidationError),
}

impl fmt::Display for LiveSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Conflict(message) | Self::Invalid(message) => {
                f.write_str(message)
            }
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::Snapshot(error) => write!(f, "{error}"),
            Self::Content(error) => write!(f, "{error}"),
            Self::Validation(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LiveSessionError {}

impl From<std::io::Error> for LiveSessionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for LiveSessionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<ContentError> for LiveSessionError {
    fn from(error: ContentError) -> Self {
        Self::Content(error)
    }
}

impl From<SnapshotError> for LiveSessionError {
    fn from(error: SnapshotError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<ValidationError> for LiveSessionError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<ApiBootstrapError> for LiveSessionError {
    fn from(error: ApiBootstrapError) -> Self {
        match error {
            ApiBootstrapError::Content(error) => Self::Content(error),
            ApiBootstrapError::Io(error) => Self::Io(error),
            ApiBootstrapError::Json(error) => Self::Json(error),
            ApiBootstrapError::Live(error) => error,
        }
    }
}

#[derive(Clone)]
pub struct SessionRegistry {
    inner: Arc<SessionRegistryInner>,
}

struct SessionRegistryInner {
    config: ApiServerConfig,
    sessions: RwLock<HashMap<SessionId, Arc<LiveSessionHandle>>>,
    next_session_id: AtomicU64,
}

impl SessionRegistry {
    pub fn load(config: ApiServerConfig) -> Result<Self, LiveSessionError> {
        fs::create_dir_all(&config.live_store_path)?;

        let mut sessions = HashMap::new();
        let mut max_session_id = 0_u64;

        for entry in fs::read_dir(&config.live_store_path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.ends_with(".meta.json") {
                continue;
            }

            let Some(session_id_text) = name.strip_suffix(".meta.json") else {
                continue;
            };
            let Ok(raw_id) = session_id_text.parse::<u64>() else {
                continue;
            };
            let session_id = SessionId::new(raw_id);
            let paths = LiveStorePaths::new(&config.live_store_path, session_id);
            let meta: PersistedSessionMeta =
                serde_json::from_str(&fs::read_to_string(&paths.meta_path)?)?;
            if meta.version != PERSISTED_SESSION_META_VERSION {
                return Err(LiveSessionError::Invalid(format!(
                    "live session metadata version {} is unsupported; expected version {}",
                    meta.version, PERSISTED_SESSION_META_VERSION
                )));
            }
            let snapshot_json = fs::read_to_string(&paths.snapshot_path)?;
            let session = GameSession::from_snapshot_json(&snapshot_json)?;
            let handle = Arc::new(LiveSessionHandle::new(
                paths,
                LiveSessionState {
                    session,
                    mode: meta.mode,
                    phase: meta.phase,
                    tick_interval_ms: meta.tick_interval_ms,
                    paused: meta.paused,
                    seats: meta.seats,
                },
            ));
            max_session_id = max_session_id.max(raw_id);
            sessions.insert(session_id, Arc::clone(&handle));
        }

        let handles = sessions.values().cloned().collect::<Vec<_>>();

        let registry = Self {
            inner: Arc::new(SessionRegistryInner {
                config,
                sessions: RwLock::new(sessions),
                next_session_id: AtomicU64::new(max_session_id.saturating_add(1).max(1)),
            }),
        };

        for handle in handles {
            handle.spawn_runner();
        }

        Ok(registry)
    }

    pub async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        let session_id = SessionId::new(self.inner.next_session_id.fetch_add(1, Ordering::SeqCst));
        let summary = load_session_summary(
            session_id,
            &self.inner.config.ruleset_path,
            &self.inner.config.scenario_path,
        )?;
        let mut seats = summary
            .scenario
            .player_ids
            .iter()
            .copied()
            .map(|player_id| PlayerSeat {
                player_id,
                claimed: false,
                ready: false,
            })
            .collect::<Vec<_>>();

        if let Some(player_id) = request.claimed_player_id {
            let seat = seats
                .iter_mut()
                .find(|seat| seat.player_id == player_id)
                .ok_or_else(|| {
                    LiveSessionError::Invalid(format!(
                        "player P{} is not part of the configured scenario",
                        player_id.0
                    ))
                })?;
            seat.claimed = true;
        }

        let handle = Arc::new(LiveSessionHandle::new(
            LiveStorePaths::new(&self.inner.config.live_store_path, session_id),
            LiveSessionState {
                session: GameSession::new(session_id, summary.config, summary.scenario),
                mode: request.mode,
                phase: SessionPhase::Lobby,
                tick_interval_ms: DEFAULT_TICK_INTERVAL_MS,
                paused: true,
                seats,
            },
        ));
        handle.persist().await?;
        handle.spawn_runner();

        self.inner
            .sessions
            .write()
            .await
            .insert(session_id, Arc::clone(&handle));

        handle.session_info().await
    }

    pub async fn session_info(
        &self,
        session_id: SessionId,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id).await?.session_info().await
    }

    pub async fn join_session(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id).await?.join(player_id).await
    }

    pub async fn set_ready(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
        ready: bool,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id)
            .await?
            .set_ready(player_id, ready)
            .await
    }

    pub async fn issue_command(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
        command: CommandKind,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id)
            .await?
            .issue_command(player_id, command)
            .await
    }

    pub async fn run_session(
        &self,
        session_id: SessionId,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id).await?.run().await
    }

    pub async fn pause_session(
        &self,
        session_id: SessionId,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id).await?.pause().await
    }

    pub async fn set_speed(
        &self,
        session_id: SessionId,
        tick_interval_ms: u64,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        self.handle(session_id)
            .await?
            .set_speed(tick_interval_ms)
            .await
    }

    pub async fn player_frame(
        &self,
        session_id: SessionId,
        player_id: PlayerId,
        from_event_index: usize,
    ) -> Result<PlayerFrameResponse, LiveSessionError> {
        self.handle(session_id)
            .await?
            .player_frame(player_id, from_event_index)
            .await
    }

    async fn handle(
        &self,
        session_id: SessionId,
    ) -> Result<Arc<LiveSessionHandle>, LiveSessionError> {
        self.inner
            .sessions
            .read()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                LiveSessionError::NotFound(format!("session {} was not found", session_id.0))
            })
    }
}

struct LiveSessionHandle {
    paths: LiveStorePaths,
    state: Mutex<LiveSessionState>,
    notify: Notify,
    runner_started: AtomicBool,
}

impl LiveSessionHandle {
    fn new(paths: LiveStorePaths, state: LiveSessionState) -> Self {
        Self {
            paths,
            state: Mutex::new(state),
            notify: Notify::new(),
            runner_started: AtomicBool::new(false),
        }
    }

    fn spawn_runner(self: &Arc<Self>) {
        if self.runner_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let handle = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let maybe_wait = {
                    let state = handle.state.lock().await;
                    if state.phase == SessionPhase::Running && !state.paused {
                        Some(state.tick_interval_ms)
                    } else {
                        None
                    }
                };

                match maybe_wait {
                    Some(wait_ms) => {
                        tokio::select! {
                            _ = sleep(Duration::from_millis(wait_ms)) => {
                                if let Err(error) = handle.advance_tick().await {
                                    eprintln!("starforge-api runner error: {error}");
                                }
                            }
                            _ = handle.notify.notified() => {}
                        }
                    }
                    None => handle.notify.notified().await,
                }
            }
        });
    }

    async fn session_info(&self) -> Result<SessionInfoResponse, LiveSessionError> {
        let state = self.state.lock().await;
        Ok(state.session_info())
    }

    async fn join(&self, player_id: PlayerId) -> Result<SessionInfoResponse, LiveSessionError> {
        let mut state = self.state.lock().await;
        let seat_index = state
            .seats
            .iter_mut()
            .position(|seat| seat.player_id == player_id)
            .ok_or_else(|| {
                LiveSessionError::Invalid(format!("player P{} is not in the session", player_id.0))
            })?;

        match state.phase {
            SessionPhase::Lobby => {
                if state.seats[seat_index].claimed {
                    return Err(LiveSessionError::Conflict(format!(
                        "player P{} already has a claimed seat in this lobby",
                        player_id.0
                    )));
                }
                state.seats[seat_index].claimed = true;
                state.seats[seat_index].ready = false;
                persist_locked(&state, &self.paths)?;
            }
            SessionPhase::Running | SessionPhase::Finished => {
                if !state.seats[seat_index].claimed {
                    return Err(LiveSessionError::Conflict(format!(
                        "player P{} must claim a seat before the match starts",
                        player_id.0
                    )));
                }
            }
        }

        Ok(state.session_info())
    }

    async fn set_ready(
        &self,
        player_id: PlayerId,
        ready: bool,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        let mut state = self.state.lock().await;
        if state.phase != SessionPhase::Lobby {
            return Err(LiveSessionError::Conflict(
                "ready status can only change while the session is in the lobby".to_owned(),
            ));
        }

        let seat_index = state
            .seats
            .iter()
            .position(|seat| seat.player_id == player_id)
            .ok_or_else(|| {
                LiveSessionError::Invalid(format!("player P{} is not in the session", player_id.0))
            })?;
        if !state.seats[seat_index].claimed {
            return Err(LiveSessionError::Conflict(format!(
                "player P{} must claim a seat before becoming ready",
                player_id.0
            )));
        }

        state.seats[seat_index].ready = ready;
        if state.mode == SessionMode::Competitive && state.all_claimed_and_ready() {
            state.phase = SessionPhase::Running;
            state.paused = false;
        }

        persist_locked(&state, &self.paths)?;
        self.notify.notify_waiters();
        Ok(state.session_info())
    }

    async fn issue_command(
        &self,
        player_id: PlayerId,
        command: CommandKind,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        let mut state = self.state.lock().await;
        if state.phase == SessionPhase::Finished {
            return Err(LiveSessionError::Conflict(
                "the session is already finished".to_owned(),
            ));
        }
        if state.mode == SessionMode::Competitive && state.phase == SessionPhase::Lobby {
            return Err(LiveSessionError::Conflict(
                "commands are disabled until the competitive match starts".to_owned(),
            ));
        }
        ensure_claimed_seat(&state.seats, player_id)?;
        state.session.issue_command_now(player_id, command)?;
        if state.session.state().victory != VictoryState::Ongoing {
            state.phase = SessionPhase::Finished;
            state.paused = true;
        }
        persist_locked(&state, &self.paths)?;
        Ok(state.session_info())
    }

    async fn run(&self) -> Result<SessionInfoResponse, LiveSessionError> {
        let mut state = self.state.lock().await;
        if state.mode != SessionMode::Sandbox {
            return Err(LiveSessionError::Conflict(
                "run is only available for sandbox sessions".to_owned(),
            ));
        }
        if state.phase == SessionPhase::Finished {
            return Err(LiveSessionError::Conflict(
                "cannot resume a finished session".to_owned(),
            ));
        }

        state.phase = SessionPhase::Running;
        state.paused = false;
        persist_locked(&state, &self.paths)?;
        self.notify.notify_waiters();
        Ok(state.session_info())
    }

    async fn pause(&self) -> Result<SessionInfoResponse, LiveSessionError> {
        let mut state = self.state.lock().await;
        if state.mode != SessionMode::Sandbox {
            return Err(LiveSessionError::Conflict(
                "pause is only available for sandbox sessions".to_owned(),
            ));
        }
        if state.phase == SessionPhase::Finished {
            return Err(LiveSessionError::Conflict(
                "cannot pause a finished session".to_owned(),
            ));
        }

        state.paused = true;
        persist_locked(&state, &self.paths)?;
        Ok(state.session_info())
    }

    async fn set_speed(
        &self,
        tick_interval_ms: u64,
    ) -> Result<SessionInfoResponse, LiveSessionError> {
        let mut state = self.state.lock().await;
        if state.mode != SessionMode::Sandbox {
            return Err(LiveSessionError::Conflict(
                "speed controls are only available for sandbox sessions".to_owned(),
            ));
        }
        if state.phase == SessionPhase::Finished {
            return Err(LiveSessionError::Conflict(
                "cannot change speed for a finished session".to_owned(),
            ));
        }
        if !ALLOWED_TICK_INTERVAL_MS.contains(&tick_interval_ms) {
            return Err(LiveSessionError::Invalid(
                "tick interval must be one of 125, 250, 500, 1000, 2500, or 5000 milliseconds"
                    .to_owned(),
            ));
        }

        state.tick_interval_ms = tick_interval_ms;
        persist_locked(&state, &self.paths)?;
        self.notify.notify_waiters();
        Ok(state.session_info())
    }

    async fn player_frame(
        &self,
        player_id: PlayerId,
        from_event_index: usize,
    ) -> Result<PlayerFrameResponse, LiveSessionError> {
        let state = self.state.lock().await;
        let view = state.session.player_view(player_id)?;
        let events = state
            .session
            .player_events_from_index(player_id, from_event_index)?;

        Ok(PlayerFrameResponse {
            session_id: state.session.session_id(),
            summary: state.summary(),
            seats: state.seats.clone(),
            runner: state.runner_status(),
            state_hash: state.session.state_hash(),
            next_event_index: state.session.event_log().len(),
            view: view.clone(),
            alerts: classify_alerts(&events),
            events,
            known_routes: known_routes(&state.session.state().connections, &view),
        })
    }

    async fn advance_tick(&self) -> Result<(), LiveSessionError> {
        let mut state = self.state.lock().await;
        if state.phase != SessionPhase::Running || state.paused {
            return Ok(());
        }

        state.session.advance_tick();
        if state.session.state().victory != VictoryState::Ongoing {
            state.phase = SessionPhase::Finished;
            state.paused = true;
        }

        persist_locked(&state, &self.paths)?;
        Ok(())
    }

    async fn persist(&self) -> Result<(), LiveSessionError> {
        let state = self.state.lock().await;
        persist_locked(&state, &self.paths)
    }
}

struct LiveSessionState {
    session: GameSession,
    mode: SessionMode,
    phase: SessionPhase,
    tick_interval_ms: u64,
    paused: bool,
    seats: Vec<PlayerSeat>,
}

impl LiveSessionState {
    fn summary(&self) -> LiveSessionSummary {
        LiveSessionSummary {
            scenario_name: self.session.scenario().name.clone(),
            current_tick: self.session.current_tick(),
            victory: self.session.state().victory.clone(),
        }
    }

    fn runner_status(&self) -> RunnerStatus {
        RunnerStatus {
            mode: self.mode.clone(),
            phase: self.phase.clone(),
            tick_interval_ms: self.tick_interval_ms,
            pause_allowed: self.mode == SessionMode::Sandbox
                && self.phase != SessionPhase::Finished,
            speed_change_allowed: self.mode == SessionMode::Sandbox
                && self.phase != SessionPhase::Finished,
            paused: self.paused,
        }
    }

    fn session_info(&self) -> SessionInfoResponse {
        SessionInfoResponse {
            session_id: self.session.session_id(),
            summary: self.summary(),
            seats: self.seats.clone(),
            runner: self.runner_status(),
            state_hash: self.session.state_hash(),
        }
    }

    fn all_claimed_and_ready(&self) -> bool {
        self.seats.iter().all(|seat| seat.claimed && seat.ready)
    }
}

struct LiveStorePaths {
    snapshot_path: PathBuf,
    meta_path: PathBuf,
}

impl LiveStorePaths {
    fn new(store_dir: &Path, session_id: SessionId) -> Self {
        Self {
            snapshot_path: store_dir.join(format!("{}.snapshot.json", session_id.0)),
            meta_path: store_dir.join(format!("{}.meta.json", session_id.0)),
        }
    }
}

fn persist_locked(
    state: &LiveSessionState,
    paths: &LiveStorePaths,
) -> Result<(), LiveSessionError> {
    let snapshot_json = state.session.snapshot_json()?;
    let meta_json = serde_json::to_string_pretty(&PersistedSessionMeta::from_state(state))?;
    write_atomic(&paths.snapshot_path, &snapshot_json)?;
    write_atomic(&paths.meta_path, &meta_json)?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("tmp")
    ));
    fs::write(&temp_path, contents)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

fn ensure_claimed_seat(seats: &[PlayerSeat], player_id: PlayerId) -> Result<(), LiveSessionError> {
    let seat = seats
        .iter()
        .find(|seat| seat.player_id == player_id)
        .ok_or_else(|| {
            LiveSessionError::Invalid(format!("player P{} is not in the session", player_id.0))
        })?;

    if !seat.claimed {
        return Err(LiveSessionError::Conflict(format!(
            "player P{} has not joined this live session",
            player_id.0
        )));
    }

    Ok(())
}

fn classify_alerts(events: &[IndexedEventRecord]) -> Vec<PlayerAlert> {
    events
        .iter()
        .filter_map(|event| match &event.record.kind {
            EventKind::TransitArrived {
                destination_id,
                kind,
                ..
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Arrival,
                title: format!("{kind:?} transit arrived at #{destination_id}"),
                tick_id: event.record.tick_id,
                location_id: Some(*destination_id),
            }),
            EventKind::LocationSurveyed { location_id } => Some(PlayerAlert {
                kind: PlayerAlertKind::Survey,
                title: format!("location #{location_id} surveyed"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::LocationClaimed { location_id, .. } => Some(PlayerAlert {
                kind: PlayerAlertKind::Claim,
                title: format!("location #{location_id} claimed"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::LocationContested { location_id, .. } => Some(PlayerAlert {
                kind: PlayerAlertKind::Contest,
                title: format!("location #{location_id} is contested"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::LocationCaptured {
                location_id,
                pacification_ticks,
                ..
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Capture,
                title: format!(
                    "location #{location_id} captured; pacification {} ticks",
                    pacification_ticks
                ),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::PacificationCompleted { location_id, .. } => Some(PlayerAlert {
                kind: PlayerAlertKind::Pacification,
                title: format!("pacification completed at #{location_id}"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::InfrastructureConditionChanged {
                location_id,
                condition,
                ..
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Repair,
                title: format!("location #{location_id} infrastructure is now {condition:?}"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::TrainingRunCompleted { achieved_tier } => Some(PlayerAlert {
                kind: PlayerAlertKind::Training,
                title: format!("training completed: tier {achieved_tier}"),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            EventKind::ResearchProjectStarted {
                branch,
                target_level,
                ..
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Research,
                title: format!(
                    "research started: {} level {}",
                    format!("{branch:?}").to_lowercase(),
                    target_level
                ),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            EventKind::ResearchProjectCompleted {
                branch,
                achieved_level,
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Research,
                title: format!(
                    "research completed: {} level {}",
                    format!("{branch:?}").to_lowercase(),
                    achieved_level
                ),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            EventKind::StrategicStrikeIntercepted { location_id, .. } => Some(PlayerAlert {
                kind: PlayerAlertKind::Strike,
                title: format!("strategic strike intercepted at #{location_id}"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::LocationDestroyed { location_id, .. } => Some(PlayerAlert {
                kind: PlayerAlertKind::Strike,
                title: format!("location #{location_id} destroyed by strategic strike"),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::AscensionStarted {
                player_id,
                location_id,
                ..
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Ascension,
                title: format!("P{} started ascension at #{location_id}", player_id.0),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::AscensionInterrupted {
                player_id,
                location_id,
                reason,
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Ascension,
                title: format!(
                    "P{} ascension interrupted at #{} ({})",
                    player_id.0, location_id, reason
                ),
                tick_id: event.record.tick_id,
                location_id: Some(*location_id),
            }),
            EventKind::CommandCollapseStarted {
                player_id,
                ticks_remaining,
            } => Some(PlayerAlert {
                kind: PlayerAlertKind::Collapse,
                title: format!(
                    "P{} command collapse started ({} ticks remaining)",
                    player_id.0, ticks_remaining
                ),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            EventKind::CommandCollapseRecovered { player_id } => Some(PlayerAlert {
                kind: PlayerAlertKind::Collapse,
                title: format!("P{} recovered from command collapse", player_id.0),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            EventKind::PlayerDefeated { player_id, reason } => Some(PlayerAlert {
                kind: PlayerAlertKind::Defeat,
                title: format!("P{} defeated ({})", player_id.0, reason),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            EventKind::VictoryDeclared { winner, .. } => Some(PlayerAlert {
                kind: PlayerAlertKind::Victory,
                title: format!("victory declared for P{}", winner.0),
                tick_id: event.record.tick_id,
                location_id: None,
            }),
            _ => None,
        })
        .collect()
}

fn known_routes(connections: &[LocationConnection], view: &PlayerStateView) -> Vec<KnownRouteView> {
    let known_location_ids = view
        .locations
        .iter()
        .filter(|location| location.visibility != LocationVisibility::Obscured)
        .map(|location| location.location_id)
        .collect::<Vec<_>>();

    connections
        .iter()
        .filter(|connection| {
            known_location_ids.contains(&connection.from_location_id)
                || known_location_ids.contains(&connection.to_location_id)
        })
        .map(|connection| KnownRouteView {
            from_location_id: connection.from_location_id,
            to_location_id: connection.to_location_id,
            travel_time_ticks: connection.travel_time_ticks,
        })
        .collect()
}
