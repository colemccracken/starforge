use starforge_content::{default_game_config, starter_scenario};
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

pub fn placeholder_session_summary() -> SessionSummary {
    SessionSummary {
        session_id: SessionId::new(1),
        scenario: starter_scenario(),
        config: default_game_config(),
    }
}
