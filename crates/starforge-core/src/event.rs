use serde::{Deserialize, Serialize};

use crate::{CommandKind, MatchSeed, PlayerId, RelayStatus, TickId, ValidationError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub tick_id: TickId,
    pub player_id: Option<PlayerId>,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    SessionCreated {
        player_ids: Vec<PlayerId>,
        seed: MatchSeed,
    },
    TickAdvanced {
        tick_id: TickId,
    },
    CommandAccepted {
        command: CommandKind,
        apply_at_tick: TickId,
    },
    CommandApplied {
        command: CommandKind,
    },
    CommandRejected {
        command: CommandKind,
        error: ValidationError,
    },
    ThroughputBudgetSet {
        reserved_for_model_upkeep: u32,
        reserved_for_training: u32,
        reserved_for_agents: u32,
        available: u32,
    },
    AgentAssigned {
        role: String,
        scope: String,
        reserved_throughput: u32,
    },
    LocationRegistered {
        location_id: u32,
        name: String,
    },
    RelayStatusChanged {
        location_id: u32,
        relay_status: RelayStatus,
    },
}
