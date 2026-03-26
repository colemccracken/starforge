use serde::{Deserialize, Serialize};

use crate::{PlayerId, SessionId, TickId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub session_id: SessionId,
    pub player_id: PlayerId,
    pub issued_at_tick: TickId,
    pub apply_at_tick: TickId,
    pub command: CommandKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandKind {
    NoOp,
    AdvanceTick,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: &'static str,
    pub message: String,
}
