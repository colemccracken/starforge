use serde::{Deserialize, Serialize};

use crate::{
    CommandEnvelope, EventRecord, GameConfig, GameState, ReplayLog, ScenarioConfig, SessionId,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub session_id: SessionId,
    pub config: GameConfig,
    pub scenario: ScenarioConfig,
    pub state: GameState,
    pub event_log: Vec<EventRecord>,
    pub replay_log: ReplayLog,
    pub pending_commands: Vec<CommandEnvelope>,
}

impl Snapshot {
    pub fn new(
        session_id: SessionId,
        config: GameConfig,
        scenario: ScenarioConfig,
        state: GameState,
        event_log: Vec<EventRecord>,
        replay_log: ReplayLog,
        pending_commands: Vec<CommandEnvelope>,
    ) -> Self {
        Self {
            version: 1,
            session_id,
            config,
            scenario,
            state,
            event_log,
            replay_log,
            pending_commands,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
