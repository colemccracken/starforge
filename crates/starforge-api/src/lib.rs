pub mod live;

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use starforge_content::{ContentError, load_compiled_scenario};
use starforge_core::{
    CommandKind, EventRecord, GameConfig, GameSession, PlayerId, PlayerStateView, ScenarioConfig,
    SessionId, TickId, ValidationError, VictoryState,
};
use starforge_taxonomy::{TaxonomyDocument, TaxonomyError, build_taxonomy_document};

pub use live::{
    CreateSessionRequest, JoinSessionRequest, KnownRouteView, LiveSessionSummary, PlayerAlert,
    PlayerAlertKind, PlayerCommandRequest, PlayerFrameQuery, PlayerFrameResponse, PlayerSeat,
    ReadySessionRequest, RunnerSpeedRequest, RunnerStatus, SessionInfoResponse, SessionMode,
    SessionPhase, SessionRegistry,
};

const TAXONOMY_HTML_TEMPLATE: &str = include_str!("../assets/taxonomy.html");
const TAXONOMY_CSS: &str = include_str!("../assets/taxonomy.css");
const TAXONOMY_JS: &str = include_str!("../assets/taxonomy.js");
const RUN_LOOP_INTERVAL_MS: u64 = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiServerConfig {
    pub bind_address: String,
    pub ruleset_path: PathBuf,
    pub scenario_path: PathBuf,
    pub live_store_path: PathBuf,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".to_owned(),
            ruleset_path: default_ruleset_path(),
            scenario_path: default_scenario_path(),
            live_store_path: default_live_store_path(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub scenario: ScenarioConfig,
    pub config: GameConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiSessionSummary {
    pub session_id: SessionId,
    pub scenario_name: String,
    pub current_tick: TickId,
    pub control_state: SessionControlState,
    pub victory: VictoryState,
    pub player_count: usize,
    pub location_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub session_id: SessionId,
    pub current_tick: TickId,
    pub control_state: SessionControlState,
    pub event_count: usize,
    pub accepted_command_count: usize,
    pub pending_command_count: usize,
    pub transit_count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionControlState {
    Running,
    #[default]
    Paused,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveSessionResponse {
    pub snapshot_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepSessionRequest {
    #[serde(default = "default_step_ticks")]
    pub ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCommandRequest {
    pub player_id: u8,
    pub command: CommandKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadSessionRequest {
    pub snapshot_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerScopedQuery {
    pub player_id: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerEventsQuery {
    pub player_id: u8,
    #[serde(default)]
    pub from_tick: u64,
}

struct ApiState {
    config: ApiServerConfig,
    sessions: Mutex<SessionStore>,
    registry: SessionRegistry,
}

#[derive(Debug)]
struct SessionStore {
    next_session_id: u64,
    sessions: BTreeMap<SessionId, ManagedSession>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self {
            next_session_id: 1,
            sessions: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
struct ManagedSession {
    session: GameSession,
    control_state: SessionControlState,
}

impl ManagedSession {
    fn new(session: GameSession) -> Self {
        Self {
            session,
            control_state: SessionControlState::Paused,
        }
    }
}

#[derive(Debug)]
pub enum ApiBootstrapError {
    Content(ContentError),
    Io(std::io::Error),
    Json(serde_json::Error),
    Live(live::LiveSessionError),
}

#[derive(Debug)]
pub enum ApiServerError {
    Bootstrap(ApiBootstrapError),
    Io(std::io::Error),
}

#[derive(Debug)]
enum ApiRouteError {
    Taxonomy(TaxonomyError),
    Bootstrap(ApiBootstrapError),
    Validation(ValidationError),
    Snapshot(serde_json::Error),
    SessionNotFound(SessionId),
    LockPoisoned,
    Live(live::LiveSessionError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ApiErrorBody {
    error: String,
    message: String,
}

impl fmt::Display for ApiBootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::Live(error) => write!(f, "{error}"),
        }
    }
}

impl fmt::Display for ApiServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap(error) => write!(f, "failed to bootstrap api server: {error}"),
            Self::Io(error) => write!(f, "failed to start api server: {error}"),
        }
    }
}

impl fmt::Display for ApiRouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Taxonomy(error) => write!(f, "{error}"),
            Self::Bootstrap(error) => write!(f, "{error}"),
            Self::Validation(error) => write!(f, "{}", error.message),
            Self::Snapshot(error) => write!(f, "failed to parse snapshot json: {error}"),
            Self::SessionNotFound(session_id) => {
                write!(f, "session {} was not found", session_id.0)
            }
            Self::LockPoisoned => write!(f, "session store lock was poisoned"),
            Self::Live(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ApiBootstrapError {}
impl std::error::Error for ApiServerError {}
impl std::error::Error for ApiRouteError {}

impl From<ContentError> for ApiBootstrapError {
    fn from(error: ContentError) -> Self {
        Self::Content(error)
    }
}

impl From<std::io::Error> for ApiBootstrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ApiBootstrapError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<live::LiveSessionError> for ApiBootstrapError {
    fn from(error: live::LiveSessionError) -> Self {
        Self::Live(error)
    }
}

impl From<TaxonomyError> for ApiRouteError {
    fn from(error: TaxonomyError) -> Self {
        Self::Taxonomy(error)
    }
}

impl From<ApiBootstrapError> for ApiRouteError {
    fn from(error: ApiBootstrapError) -> Self {
        Self::Bootstrap(error)
    }
}

impl From<ValidationError> for ApiRouteError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<serde_json::Error> for ApiRouteError {
    fn from(error: serde_json::Error) -> Self {
        Self::Snapshot(error)
    }
}

impl From<live::LiveSessionError> for ApiRouteError {
    fn from(error: live::LiveSessionError) -> Self {
        Self::Live(error)
    }
}

impl IntoResponse for ApiRouteError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::Taxonomy(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "taxonomy_error".to_owned(),
                error.to_string(),
            ),
            Self::Bootstrap(error) => (
                StatusCode::BAD_REQUEST,
                "bootstrap_error".to_owned(),
                error.to_string(),
            ),
            Self::Validation(error) => (StatusCode::BAD_REQUEST, error.code, error.message),
            Self::Snapshot(error) => (
                StatusCode::BAD_REQUEST,
                "snapshot_error".to_owned(),
                format!("failed to parse snapshot json: {error}"),
            ),
            Self::SessionNotFound(session_id) => (
                StatusCode::NOT_FOUND,
                "session_not_found".to_owned(),
                format!("session {} was not found", session_id.0),
            ),
            Self::LockPoisoned => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock_poisoned".to_owned(),
                "session store lock was poisoned".to_owned(),
            ),
            Self::Live(error) => match error {
                live::LiveSessionError::NotFound(message) => (
                    StatusCode::NOT_FOUND,
                    "live_session_not_found".to_owned(),
                    message,
                ),
                live::LiveSessionError::Conflict(message) => (
                    StatusCode::CONFLICT,
                    "live_session_conflict".to_owned(),
                    message,
                ),
                live::LiveSessionError::Invalid(message) => (
                    StatusCode::BAD_REQUEST,
                    "live_session_invalid".to_owned(),
                    message,
                ),
                live::LiveSessionError::Io(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "live_session_io".to_owned(),
                    error.to_string(),
                ),
                live::LiveSessionError::Json(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "live_session_json".to_owned(),
                    error.to_string(),
                ),
                live::LiveSessionError::Content(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "live_session_content".to_owned(),
                    error.to_string(),
                ),
                live::LiveSessionError::Validation(error) => {
                    (StatusCode::BAD_REQUEST, error.code, error.message)
                }
            },
        };

        (status, Json(ApiErrorBody { error, message })).into_response()
    }
}

pub fn planned_routes() -> &'static [&'static str] {
    &[
        "GET /",
        "GET /taxonomy",
        "GET /taxonomy.json",
        "POST /sessions",
        "POST /sessions/load",
        "GET /sessions/{id}",
        "POST /sessions/{id}/run",
        "POST /sessions/{id}/pause",
        "POST /sessions/{id}/step",
        "POST /sessions/{id}/commands",
        "GET /sessions/{id}/state",
        "GET /sessions/{id}/events",
        "POST /sessions/{id}/save",
        "GET /sessions/{id}/metrics",
        "POST /live/sessions",
        "GET /live/sessions/{id}",
        "POST /live/sessions/{id}/join",
        "POST /live/sessions/{id}/ready",
        "POST /live/sessions/{id}/commands",
        "POST /live/sessions/{id}/run",
        "POST /live/sessions/{id}/pause",
        "POST /live/sessions/{id}/speed",
        "GET /live/sessions/{id}/frame",
    ]
}

pub async fn app_router(config: ApiServerConfig) -> Result<Router, ApiBootstrapError> {
    let state = Arc::new(ApiState {
        registry: SessionRegistry::load(config.clone())?,
        sessions: Mutex::new(SessionStore::default()),
        config,
    });

    Ok(Router::new()
        .route("/", get(root))
        .route("/taxonomy", get(taxonomy_html))
        .route("/taxonomy.json", get(taxonomy_json))
        .route("/sessions", post(create_local_session))
        .route("/sessions/load", post(load_local_session))
        .route("/sessions/{id}", get(get_local_session))
        .route("/sessions/{id}/run", post(run_local_session))
        .route("/sessions/{id}/pause", post(pause_local_session))
        .route("/sessions/{id}/step", post(step_local_session))
        .route("/sessions/{id}/commands", post(issue_local_command))
        .route("/sessions/{id}/state", get(get_local_player_state))
        .route("/sessions/{id}/events", get(get_local_player_events))
        .route("/sessions/{id}/save", post(save_local_session))
        .route("/sessions/{id}/metrics", get(get_local_session_metrics))
        .route("/live/sessions", post(create_live_session))
        .route("/live/sessions/{id}", get(get_live_session))
        .route("/live/sessions/{id}/join", post(join_live_session))
        .route("/live/sessions/{id}/ready", post(set_live_ready))
        .route("/live/sessions/{id}/commands", post(issue_live_command))
        .route("/live/sessions/{id}/run", post(run_live_session))
        .route("/live/sessions/{id}/pause", post(pause_live_session))
        .route("/live/sessions/{id}/speed", post(set_live_speed))
        .route("/live/sessions/{id}/frame", get(player_live_frame))
        .with_state(state))
}

pub async fn run_server(config: ApiServerConfig) -> Result<(), ApiServerError> {
    let router = app_router(config.clone())
        .await
        .map_err(ApiServerError::Bootstrap)?;
    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .map_err(ApiServerError::Io)?;
    axum::serve(listener, router)
        .await
        .map_err(ApiServerError::Io)
}

pub fn load_session_summary(
    session_id: SessionId,
    ruleset_path: impl AsRef<FsPath>,
    scenario_path: impl AsRef<FsPath>,
) -> Result<SessionSummary, ApiBootstrapError> {
    let compiled = load_compiled_scenario(ruleset_path, scenario_path)?;

    Ok(SessionSummary {
        session_id,
        scenario: compiled.scenario_config,
        config: compiled.game_config,
    })
}

pub fn starter_session_summary() -> Result<SessionSummary, ApiBootstrapError> {
    load_session_summary(
        SessionId::new(1),
        default_ruleset_path(),
        default_scenario_path(),
    )
}

fn default_step_ticks() -> u32 {
    1
}

fn default_ruleset_path() -> PathBuf {
    workspace_root().join("content/ruleset.example.yaml")
}

fn default_scenario_path() -> PathBuf {
    workspace_root().join("scenarios/two_player_skirmish.example.yaml")
}

fn default_live_store_path() -> PathBuf {
    workspace_root().join(".starforge/live")
}

fn workspace_root() -> PathBuf {
    FsPath::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

fn player_id(value: u8) -> PlayerId {
    PlayerId::new(value)
}

fn api_session_summary(session: &ManagedSession) -> ApiSessionSummary {
    ApiSessionSummary {
        session_id: session.session.session_id(),
        scenario_name: session.session.scenario().name.clone(),
        current_tick: session.session.current_tick(),
        control_state: session.control_state,
        victory: session.session.state().victory.clone(),
        player_count: session.session.state().players.len(),
        location_count: session.session.state().locations.len(),
    }
}

fn session_metrics(session: &ManagedSession) -> SessionMetrics {
    let snapshot = session.session.snapshot();
    SessionMetrics {
        session_id: session.session.session_id(),
        current_tick: session.session.current_tick(),
        control_state: session.control_state,
        event_count: session.session.event_log().len(),
        accepted_command_count: session.session.replay_log().accepted_commands.len(),
        pending_command_count: snapshot.pending_commands.len(),
        transit_count: session.session.state().transits.len(),
    }
}

fn lock_sessions(state: &ApiState) -> Result<MutexGuard<'_, SessionStore>, ApiRouteError> {
    state
        .sessions
        .lock()
        .map_err(|_| ApiRouteError::LockPoisoned)
}

async fn root() -> Redirect {
    Redirect::temporary("/taxonomy")
}

async fn taxonomy_html() -> Html<String> {
    Html(render_taxonomy_html())
}

async fn taxonomy_json(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<TaxonomyDocument>, ApiRouteError> {
    let document =
        build_taxonomy_document(&state.config.ruleset_path, &state.config.scenario_path)?;
    Ok(Json(document))
}

async fn create_local_session(
    State(state): State<Arc<ApiState>>,
) -> Result<(StatusCode, Json<ApiSessionSummary>), ApiRouteError> {
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(sessions.next_session_id.max(1));
    sessions.next_session_id = session_id.0 + 1;
    let summary = load_session_summary(
        session_id,
        &state.config.ruleset_path,
        &state.config.scenario_path,
    )?;
    let session = ManagedSession::new(GameSession::new(
        session_id,
        summary.config,
        summary.scenario,
    ));
    let api_summary = api_session_summary(&session);
    sessions.sessions.insert(session_id, session);
    Ok((StatusCode::CREATED, Json(api_summary)))
}

async fn get_local_session(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(api_session_summary(session)))
}

async fn run_local_session(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let session_id = SessionId::new(id);
    let mut should_spawn = false;

    let summary = {
        let mut sessions = lock_sessions(&state)?;
        let session = sessions
            .sessions
            .get_mut(&session_id)
            .ok_or(ApiRouteError::SessionNotFound(session_id))?;
        if session.session.state().victory == VictoryState::Ongoing
            && session.control_state != SessionControlState::Running
        {
            session.control_state = SessionControlState::Running;
            should_spawn = true;
        }
        api_session_summary(session)
    };

    if should_spawn {
        tokio::spawn(run_session_loop(state.clone(), session_id));
    }

    Ok(Json(summary))
}

async fn pause_local_session(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get_mut(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    session.control_state = SessionControlState::Paused;
    Ok(Json(api_session_summary(session)))
}

async fn step_local_session(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<StepSessionRequest>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get_mut(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    session.session.advance_ticks(request.ticks.max(1));
    if session.session.state().victory != VictoryState::Ongoing {
        session.control_state = SessionControlState::Paused;
    }
    Ok(Json(api_session_summary(session)))
}

async fn issue_local_command(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<IssueCommandRequest>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get_mut(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    session
        .session
        .issue_command_now(player_id(request.player_id), request.command)?;
    Ok(Json(api_session_summary(session)))
}

async fn get_local_player_state(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Query(query): Query<PlayerScopedQuery>,
) -> Result<Json<PlayerStateView>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(
        session.session.player_view(player_id(query.player_id))?,
    ))
}

async fn get_local_player_events(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Query(query): Query<PlayerEventsQuery>,
) -> Result<Json<Vec<EventRecord>>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(session.session.player_events(
        player_id(query.player_id),
        TickId::new(query.from_tick),
    )?))
}

async fn save_local_session(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<SaveSessionResponse>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(SaveSessionResponse {
        snapshot_json: session.session.snapshot_json()?,
    }))
}

async fn load_local_session(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<LoadSessionRequest>,
) -> Result<(StatusCode, Json<ApiSessionSummary>), ApiRouteError> {
    let session = GameSession::from_snapshot_json(&request.snapshot_json)?;
    let session = ManagedSession::new(session);
    let summary = api_session_summary(&session);
    let session_id = session.session.session_id();
    let mut sessions = lock_sessions(&state)?;
    sessions.next_session_id = sessions.next_session_id.max(session_id.0 + 1);
    sessions.sessions.insert(session_id, session);
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn get_local_session_metrics(
    AxumPath(id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<SessionMetrics>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(session_metrics(session)))
}

async fn run_session_loop(state: Arc<ApiState>, session_id: SessionId) {
    loop {
        tokio::time::sleep(Duration::from_millis(RUN_LOOP_INTERVAL_MS)).await;

        let mut sessions = match lock_sessions(&state) {
            Ok(sessions) => sessions,
            Err(_) => return,
        };
        let Some(session) = sessions.sessions.get_mut(&session_id) else {
            return;
        };

        if session.control_state != SessionControlState::Running {
            return;
        }

        if session.session.state().victory != VictoryState::Ongoing {
            session.control_state = SessionControlState::Paused;
            return;
        }

        session.session.advance_tick();

        if session.session.state().victory != VictoryState::Ongoing {
            session.control_state = SessionControlState::Paused;
            return;
        }
    }
}

async fn create_live_session(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionInfoResponse>), ApiRouteError> {
    Ok((
        StatusCode::CREATED,
        Json(state.registry.create_session(request).await?),
    ))
}

async fn get_live_session(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .session_info(SessionId::new(session_id))
            .await?,
    ))
}

async fn join_live_session(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<JoinSessionRequest>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .join_session(SessionId::new(session_id), request.player_id)
            .await?,
    ))
}

async fn set_live_ready(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<ReadySessionRequest>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .set_ready(SessionId::new(session_id), request.player_id, request.ready)
            .await?,
    ))
}

async fn issue_live_command(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<PlayerCommandRequest>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .issue_command(
                SessionId::new(session_id),
                request.player_id,
                request.command,
            )
            .await?,
    ))
}

async fn run_live_session(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .run_session(SessionId::new(session_id))
            .await?,
    ))
}

async fn pause_live_session(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .pause_session(SessionId::new(session_id))
            .await?,
    ))
}

async fn set_live_speed(
    AxumPath(session_id): AxumPath<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<RunnerSpeedRequest>,
) -> Result<Json<SessionInfoResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .set_speed(SessionId::new(session_id), request.tick_interval_ms)
            .await?,
    ))
}

async fn player_live_frame(
    AxumPath(session_id): AxumPath<u64>,
    Query(query): Query<PlayerFrameQuery>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<PlayerFrameResponse>, ApiRouteError> {
    Ok(Json(
        state
            .registry
            .player_frame(
                SessionId::new(session_id),
                query.player_id,
                query.from_event_index,
            )
            .await?,
    ))
}

fn render_taxonomy_html() -> String {
    TAXONOMY_HTML_TEMPLATE
        .replace("/*__TAXONOMY_CSS__*/", TAXONOMY_CSS)
        .replace("//__TAXONOMY_JS__", TAXONOMY_JS)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use serde::{Serialize, de::DeserializeOwned};
    use tower::util::ServiceExt;

    use crate::live::{
        CreateSessionRequest, JoinSessionRequest, PlayerCommandRequest, PlayerFrameResponse,
        ReadySessionRequest, RunnerSpeedRequest, SessionInfoResponse,
    };
    use starforge_content::ContentError;
    use starforge_core::{CommandKind, EventKind, EventRecord, PlayerId, PlayerStateView, TickId};
    use starforge_taxonomy::TaxonomyDocument;

    use super::{
        ApiBootstrapError, ApiServerConfig, ApiSessionSummary, IssueCommandRequest,
        LoadSessionRequest, SaveSessionResponse, SessionControlState, SessionMetrics, SessionMode,
        SessionRegistry, StepSessionRequest, app_router, load_session_summary,
        starter_session_summary,
    };

    fn test_config(temp_path: &Path) -> ApiServerConfig {
        ApiServerConfig {
            live_store_path: temp_path.join("live-store"),
            ..ApiServerConfig::default()
        }
    }

    fn request(method: Method, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body)
            .expect("request should build")
    }

    fn json_body<T: Serialize>(value: &T) -> Body {
        Body::from(serde_json::to_vec(value).expect("json body should serialize"))
    }

    async fn response_json<T: DeserializeOwned>(
        app: Router,
        request: Request<Body>,
        expected_status: StatusCode,
    ) -> T {
        let response = app.oneshot(request).await.expect("router should respond");
        assert_eq!(response.status(), expected_status);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response json should deserialize")
    }

    #[test]
    fn starter_summary_uses_compiled_repo_scenario() {
        let summary = starter_session_summary().expect("starter summary should load");

        assert_eq!(summary.scenario.name, "two_player_skirmish");
        assert_eq!(summary.config.max_players, 2);
        assert!((18..=24).contains(&summary.scenario.starting_locations.len()));
        assert!(summary.scenario.connections.len() >= summary.scenario.starting_locations.len());
    }

    #[test]
    fn startup_errors_are_structured() {
        let error = load_session_summary(
            starforge_core::SessionId::new(1),
            "/tmp/missing-ruleset.yaml",
            "/tmp/missing-scenario.yaml",
        )
        .expect_err("missing files should fail");

        assert!(matches!(
            error,
            ApiBootstrapError::Content(ContentError::Io(_))
        ));
    }

    #[tokio::test]
    async fn taxonomy_json_route_returns_built_document() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let response = app_router(test_config(temp.path()))
            .await
            .expect("router should build")
            .oneshot(request(Method::GET, "/taxonomy.json", Body::empty()))
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let document: TaxonomyDocument =
            serde_json::from_slice(&body).expect("taxonomy json should deserialize");
        assert_eq!(document.ruleset_name, "starter_skirmish");
        assert_eq!(document.scenario_name, "two_player_skirmish");
    }

    #[tokio::test]
    async fn taxonomy_html_route_returns_shell() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let response = app_router(test_config(temp.path()))
            .await
            .expect("router should build")
            .oneshot(request(Method::GET, "/taxonomy", Body::empty()))
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let html = String::from_utf8(body.to_vec()).expect("html should be utf-8");
        assert!(html.contains("<title>Starforge Taxonomy</title>"));
        assert!(html.contains("fetch(\"/taxonomy.json\""));
        assert!(html.contains("id=\"entryList\""));
    }

    #[tokio::test]
    async fn session_routes_create_and_fetch_summary() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let app = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let created: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions", Body::empty()),
            StatusCode::CREATED,
        )
        .await;
        assert_eq!(created.session_id, starforge_core::SessionId::new(1));
        assert_eq!(created.scenario_name, "two_player_skirmish");

        let fetched: ApiSessionSummary = response_json(
            app,
            request(Method::GET, "/sessions/1", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(fetched, created);
    }

    #[tokio::test]
    async fn run_and_pause_routes_control_background_advancement() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let app = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions", Body::empty()),
            StatusCode::CREATED,
        )
        .await;

        let running: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions/1/run", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(running.control_state, SessionControlState::Running);

        tokio::time::sleep(Duration::from_millis(180)).await;

        let advanced: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::GET, "/sessions/1", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(advanced.control_state, SessionControlState::Running);
        assert!(advanced.current_tick.0 >= 1);

        let paused: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions/1/pause", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(paused.control_state, SessionControlState::Paused);
        let paused_tick = paused.current_tick;

        tokio::time::sleep(Duration::from_millis(120)).await;

        let settled: ApiSessionSummary = response_json(
            app,
            request(Method::GET, "/sessions/1", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(settled.control_state, SessionControlState::Paused);
        assert_eq!(settled.current_tick, paused_tick);
    }

    #[tokio::test]
    async fn session_commands_and_state_flow_through_api() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let app = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions", Body::empty()),
            StatusCode::CREATED,
        )
        .await;

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(
                Method::POST,
                "/sessions/1/commands",
                json_body(&IssueCommandRequest {
                    player_id: 1,
                    command: CommandKind::SetThroughputBudget {
                        reserved_for_model_upkeep: 0,
                        reserved_for_research: 0,
                        reserved_for_training: 20,
                        reserved_for_agents: 0,
                    },
                }),
            ),
            StatusCode::OK,
        )
        .await;

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(
                Method::POST,
                "/sessions/1/commands",
                json_body(&IssueCommandRequest {
                    player_id: 1,
                    command: CommandKind::StartTrainingRun { target_tier: 2 },
                }),
            ),
            StatusCode::OK,
        )
        .await;

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(
                Method::POST,
                "/sessions/1/step",
                json_body(&StepSessionRequest { ticks: 32 }),
            ),
            StatusCode::OK,
        )
        .await;

        let player_state: PlayerStateView = response_json(
            app.clone(),
            request(Method::GET, "/sessions/1/state?player_id=1", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(player_state.model_tier, 2);
        assert!(player_state.training.is_none());

        let events: Vec<EventRecord> = response_json(
            app,
            request(
                Method::GET,
                "/sessions/1/events?player_id=1&from_tick=0",
                Body::empty(),
            ),
            StatusCode::OK,
        )
        .await;
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            EventKind::TrainingRunStarted { target_tier: 2, .. }
        )));
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            EventKind::TrainingRunCompleted { achieved_tier: 2 }
        )));
    }

    #[tokio::test]
    async fn save_load_and_metrics_routes_round_trip_sessions() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let app = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions", Body::empty()),
            StatusCode::CREATED,
        )
        .await;

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(
                Method::POST,
                "/sessions/1/step",
                json_body(&StepSessionRequest { ticks: 3 }),
            ),
            StatusCode::OK,
        )
        .await;

        let saved: SaveSessionResponse = response_json(
            app.clone(),
            request(Method::POST, "/sessions/1/save", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert!(saved.snapshot_json.contains("\"session_id\": 1"));

        let loaded: ApiSessionSummary = response_json(
            app.clone(),
            request(
                Method::POST,
                "/sessions/load",
                json_body(&LoadSessionRequest {
                    snapshot_json: saved.snapshot_json,
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
        assert_eq!(loaded.session_id, starforge_core::SessionId::new(1));
        assert_eq!(loaded.current_tick, TickId::new(3));

        let metrics: SessionMetrics = response_json(
            app,
            request(Method::GET, "/sessions/1/metrics", Body::empty()),
            StatusCode::OK,
        )
        .await;
        assert_eq!(metrics.current_tick, TickId::new(3));
        assert!(metrics.event_count > 0);
    }

    #[tokio::test]
    async fn unknown_session_returns_not_found_json() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let response = app_router(test_config(temp.path()))
            .await
            .expect("router should build")
            .oneshot(request(Method::GET, "/sessions/999", Body::empty()))
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("error body should be json");
        assert_eq!(payload["error"], "session_not_found");
    }

    #[tokio::test]
    async fn create_join_ready_auto_starts_competitive_session() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let router = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let created: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                "/live/sessions",
                json_body(&CreateSessionRequest {
                    mode: SessionMode::Competitive,
                    claimed_player_id: Some(PlayerId::new(1)),
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
        assert_eq!(created.runner.phase, super::SessionPhase::Lobby);
        assert!(created.runner.paused);

        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/join", created.session_id.0),
                json_body(&JoinSessionRequest {
                    player_id: PlayerId::new(2),
                }),
            ),
            StatusCode::OK,
        )
        .await;

        let ready_one: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/ready", created.session_id.0),
                json_body(&ReadySessionRequest {
                    player_id: PlayerId::new(1),
                    ready: true,
                }),
            ),
            StatusCode::OK,
        )
        .await;
        assert_eq!(ready_one.runner.phase, super::SessionPhase::Lobby);

        let ready_two: SessionInfoResponse = response_json(
            router,
            request(
                Method::POST,
                &format!("/live/sessions/{}/ready", created.session_id.0),
                json_body(&ReadySessionRequest {
                    player_id: PlayerId::new(2),
                    ready: true,
                }),
            ),
            StatusCode::OK,
        )
        .await;
        assert_eq!(ready_two.runner.phase, super::SessionPhase::Running);
        assert!(!ready_two.runner.paused);
    }

    #[tokio::test]
    async fn lobby_rejects_claiming_an_already_claimed_seat() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let router = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let created: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                "/live/sessions",
                json_body(&CreateSessionRequest {
                    mode: SessionMode::Competitive,
                    claimed_player_id: Some(PlayerId::new(1)),
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

        let response = router
            .oneshot(request(
                Method::POST,
                &format!("/live/sessions/{}/join", created.session_id.0),
                json_body(&JoinSessionRequest {
                    player_id: PlayerId::new(1),
                }),
            ))
            .await
            .expect("join should respond");
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn competitive_sessions_reject_pause_and_speed_after_start() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let router = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let created: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                "/live/sessions",
                json_body(&CreateSessionRequest {
                    mode: SessionMode::Competitive,
                    claimed_player_id: Some(PlayerId::new(1)),
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/join", created.session_id.0),
                json_body(&JoinSessionRequest {
                    player_id: PlayerId::new(2),
                }),
            ),
            StatusCode::OK,
        )
        .await;
        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/ready", created.session_id.0),
                json_body(&ReadySessionRequest {
                    player_id: PlayerId::new(1),
                    ready: true,
                }),
            ),
            StatusCode::OK,
        )
        .await;
        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/ready", created.session_id.0),
                json_body(&ReadySessionRequest {
                    player_id: PlayerId::new(2),
                    ready: true,
                }),
            ),
            StatusCode::OK,
        )
        .await;

        let pause_response = router
            .clone()
            .oneshot(request(
                Method::POST,
                &format!("/live/sessions/{}/pause", created.session_id.0),
                Body::empty(),
            ))
            .await
            .expect("pause should respond");
        assert_eq!(pause_response.status(), StatusCode::CONFLICT);

        let speed_response = router
            .oneshot(request(
                Method::POST,
                &format!("/live/sessions/{}/speed", created.session_id.0),
                json_body(&RunnerSpeedRequest {
                    tick_interval_ms: 125,
                }),
            ))
            .await
            .expect("speed should respond");
        assert_eq!(speed_response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn sandbox_sessions_accept_run_pause_and_speed_changes() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let router = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let created: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                "/live/sessions",
                json_body(&CreateSessionRequest {
                    mode: SessionMode::Sandbox,
                    claimed_player_id: Some(PlayerId::new(1)),
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

        let speed_response: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/speed", created.session_id.0),
                json_body(&RunnerSpeedRequest {
                    tick_interval_ms: 5_000,
                }),
            ),
            StatusCode::OK,
        )
        .await;
        assert_eq!(speed_response.runner.tick_interval_ms, 5_000);

        let run_response: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/run", created.session_id.0),
                Body::empty(),
            ),
            StatusCode::OK,
        )
        .await;
        assert_eq!(run_response.runner.phase, super::SessionPhase::Running);
        assert!(!run_response.runner.paused);

        let pause_response: SessionInfoResponse = response_json(
            router,
            request(
                Method::POST,
                &format!("/live/sessions/{}/pause", created.session_id.0),
                Body::empty(),
            ),
            StatusCode::OK,
        )
        .await;
        assert!(pause_response.runner.paused);
    }

    #[tokio::test]
    async fn persisted_snapshot_and_meta_restore_runner_state() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let config = test_config(temp.path());
        let registry = SessionRegistry::load(config.clone()).expect("registry should load");

        let created = registry
            .create_session(CreateSessionRequest {
                mode: SessionMode::Sandbox,
                claimed_player_id: Some(PlayerId::new(1)),
            })
            .await
            .expect("session should create");
        registry
            .set_speed(created.session_id, 125)
            .await
            .expect("speed should update");
        registry
            .run_session(created.session_id)
            .await
            .expect("sandbox session should start");
        registry
            .issue_command(
                created.session_id,
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 17,
                },
            )
            .await
            .expect("command should apply");

        tokio::time::sleep(tokio::time::Duration::from_millis(275)).await;
        let before = registry
            .session_info(created.session_id)
            .await
            .expect("session should exist");

        let restored = SessionRegistry::load(config)
            .expect("restored registry should load")
            .session_info(created.session_id)
            .await
            .expect("restored session should exist");

        assert_eq!(restored.state_hash, before.state_hash);
        assert_eq!(restored.summary.current_tick, before.summary.current_tick);
        assert_eq!(restored.runner.phase, before.runner.phase);
        assert_eq!(restored.seats, before.seats);
    }

    #[tokio::test]
    async fn live_frame_reports_arrival_alerts_with_monotonic_event_cursor() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let router = app_router(test_config(temp.path()))
            .await
            .expect("router should build");

        let created: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                "/live/sessions",
                json_body(&CreateSessionRequest {
                    mode: SessionMode::Competitive,
                    claimed_player_id: Some(PlayerId::new(1)),
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/join", created.session_id.0),
                json_body(&JoinSessionRequest {
                    player_id: PlayerId::new(2),
                }),
            ),
            StatusCode::OK,
        )
        .await;
        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/ready", created.session_id.0),
                json_body(&ReadySessionRequest {
                    player_id: PlayerId::new(1),
                    ready: true,
                }),
            ),
            StatusCode::OK,
        )
        .await;
        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/ready", created.session_id.0),
                json_body(&ReadySessionRequest {
                    player_id: PlayerId::new(2),
                    ready: true,
                }),
            ),
            StatusCode::OK,
        )
        .await;

        let _: SessionInfoResponse = response_json(
            router.clone(),
            request(
                Method::POST,
                &format!("/live/sessions/{}/commands", created.session_id.0),
                json_body(&PlayerCommandRequest {
                    player_id: PlayerId::new(1),
                    command: CommandKind::DispatchSurveyTransit {
                        origin_location_id: 1,
                        destination_location_id: 17,
                    },
                }),
            ),
            StatusCode::OK,
        )
        .await;

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(20);
        let frame = loop {
            let frame: PlayerFrameResponse = response_json(
                router.clone(),
                request(
                    Method::GET,
                    &format!(
                        "/live/sessions/{}/frame?player_id=1&from_event_index=0",
                        created.session_id.0
                    ),
                    Body::empty(),
                ),
                StatusCode::OK,
            )
            .await;

            let saw_arrival = frame.alerts.iter().any(|alert| {
                alert.kind == super::live::PlayerAlertKind::Arrival && alert.location_id == Some(17)
            });
            let saw_survey = frame.alerts.iter().any(|alert| {
                alert.kind == super::live::PlayerAlertKind::Survey && alert.location_id == Some(17)
            });
            if saw_arrival && saw_survey {
                break frame;
            }

            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for arrival+survey alerts; last alerts: {:?}",
                frame.alerts
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        };

        assert!(frame.next_event_index >= frame.events.len());
        assert!(frame.alerts.iter().any(|alert| {
            alert.kind == super::live::PlayerAlertKind::Arrival && alert.location_id == Some(17)
        }));
        assert!(frame.alerts.iter().any(|alert| {
            alert.kind == super::live::PlayerAlertKind::Survey && alert.location_id == Some(17)
        }));

        let follow_up: PlayerFrameResponse = response_json(
            router,
            request(
                Method::GET,
                &format!(
                    "/live/sessions/{}/frame?player_id=1&from_event_index={}",
                    created.session_id.0, frame.next_event_index
                ),
                Body::empty(),
            ),
            StatusCode::OK,
        )
        .await;
        assert!(follow_up.events.is_empty());
        assert_eq!(follow_up.next_event_index, frame.next_event_index);
    }
}
