use serde::{Deserialize, Serialize};

use crate::{PlayerId, SessionId, TickId};

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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: &'static str,
    pub message: String,
}
