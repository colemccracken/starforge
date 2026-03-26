use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{InfrastructureKind, PlayerId, RelayStatus, SessionId, TickId};

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
    QueueInfrastructureRepair {
        location_id: u32,
        infrastructure_kind: InfrastructureKind,
    },
    QueueInfrastructureConstruction {
        location_id: u32,
        infrastructure_kind: InfrastructureKind,
    },
    DispatchSurveyTransit {
        origin_location_id: u32,
        destination_location_id: u32,
    },
    DispatchPacificationTransit {
        origin_location_id: u32,
        destination_location_id: u32,
    },
    DispatchClaimTransit {
        origin_location_id: u32,
        destination_location_id: u32,
    },
    SurveyLocation {
        location_id: u32,
    },
    StartTrainingRun {
        target_tier: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for ValidationError {}
