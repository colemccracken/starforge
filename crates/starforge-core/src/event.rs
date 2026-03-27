use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoStaticStr};

use crate::{
    CommandKind, InfrastructureCondition, InfrastructureKind, MatchSeed, PlayerId, RelayStatus,
    ResourceStockpiles, TickId, TransitKind, ValidationError,
};

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
    EconomyUpdated {
        player_id: PlayerId,
        total_connected_energy: u32,
        total_connected_datacenter_capacity: u32,
        usable_throughput: u32,
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
    InfrastructureConditionChanged {
        location_id: u32,
        kind: InfrastructureKind,
        condition: InfrastructureCondition,
    },
    InfrastructureRepairQueued {
        location_id: u32,
        kind: InfrastructureKind,
        duration_ticks: u32,
        cost: ResourceStockpiles,
    },
    InfrastructureRepairCompleted {
        location_id: u32,
        kind: InfrastructureKind,
    },
    InfrastructureConstructionQueued {
        location_id: u32,
        kind: InfrastructureKind,
        duration_ticks: u32,
        cost: ResourceStockpiles,
    },
    InfrastructureConstructionCompleted {
        location_id: u32,
        kind: InfrastructureKind,
    },
    TransitDispatched {
        transit_id: u32,
        origin_id: u32,
        destination_id: u32,
        eta_tick: TickId,
        kind: TransitKind,
    },
    TransitArrived {
        transit_id: u32,
        destination_id: u32,
        kind: TransitKind,
    },
    LocationSurveyed {
        location_id: u32,
    },
    HostileRemnantCleared {
        location_id: u32,
    },
    LocationClaimed {
        location_id: u32,
        player_id: PlayerId,
    },
    TrainingRunStarted {
        target_tier: u8,
        required_training_throughput: u32,
        required_ticks: u32,
    },
    TrainingRunCompleted {
        achieved_tier: u8,
    },
    VictoryDeclared {
        winner: PlayerId,
        reason: String,
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
pub enum EventDiscriminant {
    SessionCreated,
    TickAdvanced,
    CommandAccepted,
    CommandApplied,
    CommandRejected,
    ThroughputBudgetSet,
    EconomyUpdated,
    AgentAssigned,
    LocationRegistered,
    RelayStatusChanged,
    InfrastructureConditionChanged,
    InfrastructureRepairQueued,
    InfrastructureRepairCompleted,
    InfrastructureConstructionQueued,
    InfrastructureConstructionCompleted,
    TransitDispatched,
    TransitArrived,
    LocationSurveyed,
    HostileRemnantCleared,
    LocationClaimed,
    TrainingRunStarted,
    TrainingRunCompleted,
    VictoryDeclared,
}

impl EventDiscriminant {
    pub fn implementation_key(self) -> String {
        let discriminant: &'static str = self.into();
        format!("event_kind.{discriminant}")
    }
}

impl From<&EventKind> for EventDiscriminant {
    fn from(value: &EventKind) -> Self {
        match value {
            EventKind::SessionCreated { .. } => Self::SessionCreated,
            EventKind::TickAdvanced { .. } => Self::TickAdvanced,
            EventKind::CommandAccepted { .. } => Self::CommandAccepted,
            EventKind::CommandApplied { .. } => Self::CommandApplied,
            EventKind::CommandRejected { .. } => Self::CommandRejected,
            EventKind::ThroughputBudgetSet { .. } => Self::ThroughputBudgetSet,
            EventKind::EconomyUpdated { .. } => Self::EconomyUpdated,
            EventKind::AgentAssigned { .. } => Self::AgentAssigned,
            EventKind::LocationRegistered { .. } => Self::LocationRegistered,
            EventKind::RelayStatusChanged { .. } => Self::RelayStatusChanged,
            EventKind::InfrastructureConditionChanged { .. } => {
                Self::InfrastructureConditionChanged
            }
            EventKind::InfrastructureRepairQueued { .. } => Self::InfrastructureRepairQueued,
            EventKind::InfrastructureRepairCompleted { .. } => Self::InfrastructureRepairCompleted,
            EventKind::InfrastructureConstructionQueued { .. } => {
                Self::InfrastructureConstructionQueued
            }
            EventKind::InfrastructureConstructionCompleted { .. } => {
                Self::InfrastructureConstructionCompleted
            }
            EventKind::TransitDispatched { .. } => Self::TransitDispatched,
            EventKind::TransitArrived { .. } => Self::TransitArrived,
            EventKind::LocationSurveyed { .. } => Self::LocationSurveyed,
            EventKind::HostileRemnantCleared { .. } => Self::HostileRemnantCleared,
            EventKind::LocationClaimed { .. } => Self::LocationClaimed,
            EventKind::TrainingRunStarted { .. } => Self::TrainingRunStarted,
            EventKind::TrainingRunCompleted { .. } => Self::TrainingRunCompleted,
            EventKind::VictoryDeclared { .. } => Self::VictoryDeclared,
        }
    }
}
