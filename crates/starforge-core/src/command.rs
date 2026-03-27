use std::fmt;

use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoStaticStr};

use crate::{InfrastructureKind, PlayerId, RelayStatus, ResearchBranch, SessionId, TickId};

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
        reserved_for_research: u32,
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
    DispatchAssaultTransit {
        origin_location_id: u32,
        destination_location_id: u32,
    },
    DispatchStrategicStrike {
        origin_location_id: u32,
        destination_location_id: u32,
    },
    SurveyLocation {
        location_id: u32,
    },
    StartTrainingRun {
        target_tier: u8,
    },
    StartResearchProject {
        branch: ResearchBranch,
        target_level: u8,
    },
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum CommandDiscriminant {
    NoOp,
    AdvanceTick,
    SetThroughputBudget,
    AssignAgent,
    RegisterLocation,
    SetRelayStatus,
    QueueInfrastructureRepair,
    QueueInfrastructureConstruction,
    DispatchSurveyTransit,
    DispatchPacificationTransit,
    DispatchClaimTransit,
    DispatchAssaultTransit,
    DispatchStrategicStrike,
    SurveyLocation,
    StartTrainingRun,
    StartResearchProject,
}

impl CommandDiscriminant {
    pub fn implementation_key(self) -> String {
        let discriminant: &'static str = self.into();
        format!("command_kind.{discriminant}")
    }
}

impl From<&CommandKind> for CommandDiscriminant {
    fn from(value: &CommandKind) -> Self {
        match value {
            CommandKind::NoOp => Self::NoOp,
            CommandKind::AdvanceTick => Self::AdvanceTick,
            CommandKind::SetThroughputBudget { .. } => Self::SetThroughputBudget,
            CommandKind::AssignAgent { .. } => Self::AssignAgent,
            CommandKind::RegisterLocation { .. } => Self::RegisterLocation,
            CommandKind::SetRelayStatus { .. } => Self::SetRelayStatus,
            CommandKind::QueueInfrastructureRepair { .. } => Self::QueueInfrastructureRepair,
            CommandKind::QueueInfrastructureConstruction { .. } => {
                Self::QueueInfrastructureConstruction
            }
            CommandKind::DispatchSurveyTransit { .. } => Self::DispatchSurveyTransit,
            CommandKind::DispatchPacificationTransit { .. } => Self::DispatchPacificationTransit,
            CommandKind::DispatchClaimTransit { .. } => Self::DispatchClaimTransit,
            CommandKind::DispatchAssaultTransit { .. } => Self::DispatchAssaultTransit,
            CommandKind::DispatchStrategicStrike { .. } => Self::DispatchStrategicStrike,
            CommandKind::SurveyLocation { .. } => Self::SurveyLocation,
            CommandKind::StartTrainingRun { .. } => Self::StartTrainingRun,
            CommandKind::StartResearchProject { .. } => Self::StartResearchProject,
        }
    }
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
