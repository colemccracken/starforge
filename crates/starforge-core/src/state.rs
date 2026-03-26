use serde::{Deserialize, Serialize};

use crate::{MatchSeed, PlayerId, StartingLocation, TickId};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub tick_id: TickId,
    pub rng_state: u64,
    pub players: Vec<PlayerState>,
    pub locations: Vec<LocationState>,
    pub transits: Vec<TransitState>,
    pub victory: VictoryState,
}

impl GameState {
    pub fn new(
        player_ids: Vec<PlayerId>,
        seed: MatchSeed,
        starting_locations: Vec<StartingLocation>,
    ) -> Self {
        Self {
            tick_id: TickId::default(),
            rng_state: seed.as_u64(),
            players: player_ids.into_iter().map(PlayerState::new).collect(),
            locations: starting_locations
                .into_iter()
                .map(LocationState::from)
                .collect(),
            transits: Vec::new(),
            victory: VictoryState::Ongoing,
        }
    }

    pub fn next_random_u64(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_add(0x9e3779b97f4a7c15);

        let mut z = self.rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);

        z ^ (z >> 31)
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
    pub kind: LocationKind,
    pub territory: TerritoryState,
    pub controller: Option<PlayerId>,
    pub homeworld_of: Option<PlayerId>,
    pub relay_status: RelayStatus,
    pub orbital_slots: u8,
    pub has_environmental_hazard: bool,
    pub hostile_remnant_present: bool,
}

impl From<StartingLocation> for LocationState {
    fn from(location: StartingLocation) -> Self {
        Self {
            location_id: location.location_id,
            name: location.name,
            kind: location.kind,
            territory: location.territory,
            controller: location.controller,
            homeworld_of: location.homeworld_of,
            relay_status: location.relay_status,
            orbital_slots: location.orbital_slots,
            has_environmental_hazard: location.has_environmental_hazard,
            hostile_remnant_present: location.hostile_remnant_present,
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LocationKind {
    HabitablePlanet,
    BarrenWorld,
    VolcanicWorld,
    IceWorld,
    Moon,
    AsteroidCluster,
    GasGiant,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TerritoryState {
    Neutral,
    Owned,
    Contested,
    Destroyed,
    Obscured,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
