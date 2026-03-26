use serde::{Deserialize, Serialize};

use crate::{PlayerId, TickId};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub tick_id: TickId,
    pub players: Vec<PlayerState>,
    pub locations: Vec<LocationState>,
    pub transits: Vec<TransitState>,
    pub victory: VictoryState,
}

impl GameState {
    pub fn new(player_ids: Vec<PlayerId>) -> Self {
        Self {
            tick_id: TickId::default(),
            players: player_ids.into_iter().map(PlayerState::new).collect(),
            locations: Vec::new(),
            transits: Vec::new(),
            victory: VictoryState::Ongoing,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub player_id: PlayerId,
    pub throughput: ThroughputBudget,
    pub visibility: VisibilityState,
    pub training: Option<TrainingRunState>,
    pub collapse: CommandCollapseState,
    pub agents: Vec<AgentAssignment>,
}

impl PlayerState {
    pub fn new(player_id: PlayerId) -> Self {
        Self {
            player_id,
            throughput: ThroughputBudget::default(),
            visibility: VisibilityState::default(),
            training: None,
            collapse: CommandCollapseState::Stable,
            agents: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationState {
    pub location_id: u32,
    pub name: String,
    pub relay_status: RelayStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitState {
    pub transit_id: u32,
    pub origin_id: u32,
    pub destination_id: u32,
    pub eta_tick: TickId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibilityState {
    pub observed_location_ids: Vec<u32>,
    pub contested_location_ids: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThroughputBudget {
    pub reserved_for_model_upkeep: u32,
    pub reserved_for_training: u32,
    pub reserved_for_agents: u32,
    pub available: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingRunState {
    pub tier_name: String,
    pub progress_ticks: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAssignment {
    pub role: String,
    pub scope: String,
    pub reserved_throughput: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayStatus {
    #[default]
    Connected,
    Disconnected,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandCollapseState {
    #[default]
    Stable,
    Collapsing {
        ticks_remaining: u64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VictoryState {
    #[default]
    Ongoing,
    Won {
        winner: PlayerId,
    },
}
