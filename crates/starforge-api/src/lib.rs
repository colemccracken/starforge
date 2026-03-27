use std::{
    collections::BTreeMap,
    fmt,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
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

const TAXONOMY_HTML_TEMPLATE: &str = include_str!("../assets/taxonomy.html");
const TAXONOMY_CSS: &str = include_str!("../assets/taxonomy.css");
const TAXONOMY_JS: &str = include_str!("../assets/taxonomy.js");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiServerConfig {
    pub bind_address: String,
    pub ruleset_path: PathBuf,
    pub scenario_path: PathBuf,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".to_owned(),
            ruleset_path: default_ruleset_path(),
            scenario_path: default_scenario_path(),
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
    pub victory: VictoryState,
    pub player_count: usize,
    pub location_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub session_id: SessionId,
    pub current_tick: TickId,
    pub event_count: usize,
    pub accepted_command_count: usize,
    pub pending_command_count: usize,
    pub transit_count: usize,
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

#[derive(Debug)]
struct ApiState {
    config: ApiServerConfig,
    sessions: Mutex<SessionStore>,
}

#[derive(Debug)]
struct SessionStore {
    next_session_id: u64,
    sessions: BTreeMap<SessionId, GameSession>,
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
pub enum ApiBootstrapError {
    Content(ContentError),
}

#[derive(Debug)]
pub enum ApiServerError {
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
        }
    }
}

impl fmt::Display for ApiServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
        "GET /sessions/{id}",
        "POST /sessions/{id}/run",
        "POST /sessions/{id}/pause",
        "POST /sessions/{id}/step",
        "POST /sessions/{id}/commands",
        "GET /sessions/{id}/state",
        "GET /sessions/{id}/events",
        "POST /sessions/{id}/save",
        "POST /sessions/load",
        "GET /sessions/{id}/metrics",
    ]
}

pub fn app_router(config: ApiServerConfig) -> Router {
    let state = Arc::new(ApiState {
        config,
        sessions: Mutex::new(SessionStore::default()),
    });

    Router::new()
        .route("/", get(root))
        .route("/taxonomy", get(taxonomy_html))
        .route("/taxonomy.json", get(taxonomy_json))
        .route("/sessions", post(create_session))
        .route("/sessions/load", post(load_session))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/step", post(step_session))
        .route("/sessions/{id}/commands", post(issue_command))
        .route("/sessions/{id}/state", get(get_player_state))
        .route("/sessions/{id}/events", get(get_player_events))
        .route("/sessions/{id}/save", post(save_session))
        .route("/sessions/{id}/metrics", get(get_session_metrics))
        .with_state(state)
}

pub async fn run_server(config: ApiServerConfig) -> Result<(), ApiServerError> {
    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .map_err(ApiServerError::Io)?;
    axum::serve(listener, app_router(config))
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

fn workspace_root() -> PathBuf {
    FsPath::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

fn player_id(value: u8) -> PlayerId {
    PlayerId::new(value)
}

fn api_session_summary(session: &GameSession) -> ApiSessionSummary {
    ApiSessionSummary {
        session_id: session.session_id(),
        scenario_name: session.scenario().name.clone(),
        current_tick: session.current_tick(),
        victory: session.state().victory.clone(),
        player_count: session.state().players.len(),
        location_count: session.state().locations.len(),
    }
}

fn session_metrics(session: &GameSession) -> SessionMetrics {
    let snapshot = session.snapshot();
    SessionMetrics {
        session_id: session.session_id(),
        current_tick: session.current_tick(),
        event_count: session.event_log().len(),
        accepted_command_count: session.replay_log().accepted_commands.len(),
        pending_command_count: snapshot.pending_commands.len(),
        transit_count: session.state().transits.len(),
    }
}

fn lock_sessions(state: &ApiState) -> Result<MutexGuard<'_, SessionStore>, ApiRouteError> {
    state
        .sessions
        .lock()
        .map_err(|_| ApiRouteError::LockPoisoned)
}

fn bootstrap_session(config: &ApiServerConfig) -> Result<GameSession, ApiRouteError> {
    let compiled = load_compiled_scenario(&config.ruleset_path, &config.scenario_path)
        .map_err(ApiBootstrapError::from)?;
    Ok(GameSession::new(
        SessionId::default(),
        compiled.game_config,
        compiled.scenario_config,
    ))
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

async fn create_session(
    State(state): State<Arc<ApiState>>,
) -> Result<(StatusCode, Json<ApiSessionSummary>), ApiRouteError> {
    let mut session = bootstrap_session(&state.config)?;
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(sessions.next_session_id.max(1));
    sessions.next_session_id = session_id.0 + 1;
    session = GameSession::new(
        session_id,
        session.config().clone(),
        session.scenario().clone(),
    );
    let summary = api_session_summary(&session);
    sessions.sessions.insert(session_id, session);
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn get_session(
    Path(id): Path<u64>,
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

async fn step_session(
    Path(id): Path<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<StepSessionRequest>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get_mut(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    session.advance_ticks(request.ticks.max(1));
    Ok(Json(api_session_summary(session)))
}

async fn issue_command(
    Path(id): Path<u64>,
    State(state): State<Arc<ApiState>>,
    Json(request): Json<IssueCommandRequest>,
) -> Result<Json<ApiSessionSummary>, ApiRouteError> {
    let mut sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get_mut(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    session.issue_command_now(player_id(request.player_id), request.command)?;
    Ok(Json(api_session_summary(session)))
}

async fn get_player_state(
    Path(id): Path<u64>,
    State(state): State<Arc<ApiState>>,
    Query(query): Query<PlayerScopedQuery>,
) -> Result<Json<PlayerStateView>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(session.player_view(player_id(query.player_id))?))
}

async fn get_player_events(
    Path(id): Path<u64>,
    State(state): State<Arc<ApiState>>,
    Query(query): Query<PlayerEventsQuery>,
) -> Result<Json<Vec<EventRecord>>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(session.player_events(
        player_id(query.player_id),
        TickId::new(query.from_tick),
    )?))
}

async fn save_session(
    Path(id): Path<u64>,
    State(state): State<Arc<ApiState>>,
) -> Result<Json<SaveSessionResponse>, ApiRouteError> {
    let sessions = lock_sessions(&state)?;
    let session_id = SessionId::new(id);
    let session = sessions
        .sessions
        .get(&session_id)
        .ok_or(ApiRouteError::SessionNotFound(session_id))?;
    Ok(Json(SaveSessionResponse {
        snapshot_json: session.snapshot_json()?,
    }))
}

async fn load_session(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<LoadSessionRequest>,
) -> Result<(StatusCode, Json<ApiSessionSummary>), ApiRouteError> {
    let session = GameSession::from_snapshot_json(&request.snapshot_json)?;
    let summary = api_session_summary(&session);
    let session_id = session.session_id();
    let mut sessions = lock_sessions(&state)?;
    sessions.next_session_id = sessions.next_session_id.max(session_id.0 + 1);
    sessions.sessions.insert(session_id, session);
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn get_session_metrics(
    Path(id): Path<u64>,
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

fn render_taxonomy_html() -> String {
    TAXONOMY_HTML_TEMPLATE
        .replace("/*__TAXONOMY_CSS__*/", TAXONOMY_CSS)
        .replace("//__TAXONOMY_JS__", TAXONOMY_JS)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use tower::util::ServiceExt;

    use super::{
        ApiBootstrapError, ApiServerConfig, ApiSessionSummary, LoadSessionRequest,
        SaveSessionResponse, SessionMetrics, StepSessionRequest, app_router, load_session_summary,
        starter_session_summary,
    };
    use starforge_content::ContentError;
    use starforge_core::{EventKind, EventRecord, PlayerStateView, TickId};
    use starforge_taxonomy::TaxonomyDocument;

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
        let response = app_router(ApiServerConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/taxonomy.json")
                    .body(Body::empty())
                    .unwrap(),
            )
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
        let response = app_router(ApiServerConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/taxonomy")
                    .body(Body::empty())
                    .unwrap(),
            )
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
        let app = app_router(ApiServerConfig::default());

        let created: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions", Body::empty()),
            StatusCode::CREATED,
        )
        .await;

        assert_eq!(created.session_id, starforge_core::SessionId::new(1));
        assert_eq!(created.scenario_name, "two_player_skirmish");

        let fetched: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::GET, "/sessions/1", Body::empty()),
            StatusCode::OK,
        )
        .await;

        assert_eq!(fetched, created);
    }

    #[tokio::test]
    async fn session_commands_and_state_flow_through_api() {
        let app = app_router(ApiServerConfig::default());
        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions", Body::empty()),
            StatusCode::CREATED,
        )
        .await;

        let budget_body = json_body(&serde_json::json!({
            "player_id": 1,
            "command": {
                "SetThroughputBudget": {
                    "reserved_for_model_upkeep": 0,
                    "reserved_for_training": 20,
                    "reserved_for_agents": 0
                }
            }
        }));
        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions/1/commands", budget_body),
            StatusCode::OK,
        )
        .await;

        let train_body = json_body(&serde_json::json!({
            "player_id": 1,
            "command": {
                "StartTrainingRun": {
                    "target_tier": 2
                }
            }
        }));
        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(Method::POST, "/sessions/1/commands", train_body),
            StatusCode::OK,
        )
        .await;

        let _: ApiSessionSummary = response_json(
            app.clone(),
            request(
                Method::POST,
                "/sessions/1/step",
                json_body(&StepSessionRequest { ticks: 8 }),
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
            app.clone(),
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
        let app = app_router(ApiServerConfig::default());
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
        let response = app_router(ApiServerConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/sessions/999")
                    .body(Body::empty())
                    .unwrap(),
            )
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
}
