use serde::{Deserialize, Serialize};

use crate::{PlayerId, RelayStatus, SessionId, TickId};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub session_id: SessionId,
    pub player_id: PlayerId,
    pub issued_at_tick: TickId,
    pub apply_at_tick: TickId,
    pub command: CommandKind,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CommandKind {
    NoOp,
    AdvanceTick,
    SetThroughputBudget {
        reserved_for_model_upkeep: u32,
        reserved_for_training: u32,
        reserved_for_agents: u32,
        available: u32,
    },
    AssignAgent {
        role: String,
        scope: String,
        reserved_throughput: u32,
    },
    RegisterLocation {
        location_id: u32,
        name: String,
    },
    SetRelayStatus {
        location_id: u32,
        relay_status: RelayStatus,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
}
