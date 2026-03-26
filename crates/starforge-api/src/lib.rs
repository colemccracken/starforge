use std::{
    fmt,
    path::{Path, PathBuf},
};

use starforge_content::{ContentError, load_compiled_scenario};
use starforge_core::{GameConfig, ScenarioConfig, SessionId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiServerConfig {
    pub bind_address: String,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub scenario: ScenarioConfig,
    pub config: GameConfig,
}

#[derive(Debug)]
pub enum ApiBootstrapError {
    Content(ContentError),
}

impl fmt::Display for ApiBootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ApiBootstrapError {}

impl From<ContentError> for ApiBootstrapError {
    fn from(error: ContentError) -> Self {
        Self::Content(error)
    }
}

pub fn planned_routes() -> &'static [&'static str] {
    &[
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

#[cfg(test)]
mod tests {
    use super::{ApiBootstrapError, starter_session_summary};
    use starforge_content::ContentError;

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
}
