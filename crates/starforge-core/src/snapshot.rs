use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    CommandEnvelope, EventRecord, GameConfig, GameState, ReplayLog, ScenarioConfig, SessionId,
};

pub const SNAPSHOT_VERSION: u32 = 2;

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

#[derive(Debug)]
pub enum SnapshotError {
    Json(serde_json::Error),
    MissingVersion,
    UnsupportedVersion(u32),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "{error}"),
            Self::MissingVersion => write!(f, "snapshot is missing a version field"),
            Self::UnsupportedVersion(version) => write!(
                f,
                "snapshot version {version} is unsupported; expected version {SNAPSHOT_VERSION}"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<serde_json::Error> for SnapshotError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
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
            version: SNAPSHOT_VERSION,
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

    pub fn from_json(json: &str) -> Result<Self, SnapshotError> {
        let value: Value = serde_json::from_str(json)?;
        let Some(version) = value.get("version").and_then(Value::as_u64) else {
            return Err(SnapshotError::MissingVersion);
        };
        if version != u64::from(SNAPSHOT_VERSION) {
            return Err(SnapshotError::UnsupportedVersion(version as u32));
        }

        serde_json::from_value(value).map_err(SnapshotError::from)
    }
}
