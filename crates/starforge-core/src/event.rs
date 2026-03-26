use serde::{Deserialize, Serialize};

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
