use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use starforge_content::{ContentError, load_compiled_scenario};
use starforge_core::{GameConfig, ScenarioConfig, SessionId};
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

#[derive(Clone, Debug)]
struct ApiState {
    config: ApiServerConfig,
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

impl IntoResponse for ApiRouteError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            self.to_string(),
        )
            .into_response()
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
    let state = Arc::new(ApiState { config });

    Router::new()
        .route("/", get(root))
        .route("/taxonomy", get(taxonomy_html))
        .route("/taxonomy.json", get(taxonomy_json))
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
    ruleset_path: impl AsRef<Path>,
    scenario_path: impl AsRef<Path>,
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

fn default_ruleset_path() -> PathBuf {
    workspace_root().join("content/ruleset.example.yaml")
}

fn default_scenario_path() -> PathBuf {
    workspace_root().join("scenarios/two_player_skirmish.example.yaml")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
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

fn render_taxonomy_html() -> String {
    TAXONOMY_HTML_TEMPLATE
        .replace("/*__TAXONOMY_CSS__*/", TAXONOMY_CSS)
        .replace("//__TAXONOMY_JS__", TAXONOMY_JS)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    use super::{ApiBootstrapError, app_router, starter_session_summary};
    use starforge_content::ContentError;
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
        let error = super::load_session_summary(
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
        let response = app_router(super::ApiServerConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/taxonomy.json")
                    .body(Body::empty())
                    .expect("request should build"),
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
        assert!(
            document
                .entries
                .iter()
                .any(|entry| entry.id == "economy.throughput")
        );
    }

    #[tokio::test]
    async fn taxonomy_html_route_returns_shell() {
        let response = app_router(super::ApiServerConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/taxonomy")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let html = String::from_utf8(body.to_vec()).expect("html should be utf-8");
        assert!(html.contains("Starforge Taxonomy"));
        assert!(html.contains("/taxonomy.json"));
    }
}
