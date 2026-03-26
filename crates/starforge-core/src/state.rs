use serde::{Deserialize, Serialize};

use crate::{LocationConnection, MatchSeed, PlayerId, StartingLocation, TickId};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub tick_id: TickId,
    pub rng_state: u64,
    pub players: Vec<PlayerState>,
    pub locations: Vec<LocationState>,
    pub connections: Vec<LocationConnection>,
    pub transits: Vec<TransitState>,
    pub victory: VictoryState,
}

impl GameState {
    pub fn new(
        player_ids: Vec<PlayerId>,
        seed: MatchSeed,
        starting_locations: Vec<StartingLocation>,
        connections: Vec<LocationConnection>,
    ) -> Self {
        let mut state = Self {
            tick_id: TickId::default(),
            rng_state: seed.as_u64(),
            players: player_ids.into_iter().map(PlayerState::new).collect(),
            locations: starting_locations
                .into_iter()
                .map(LocationState::from)
                .collect(),
            connections,
            transits: Vec::new(),
            victory: VictoryState::Ongoing,
        };
        state.recompute_economy();
        state
    }

    pub fn next_random_u64(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_add(0x9e3779b97f4a7c15);

        let mut z = self.rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);

        z ^ (z >> 31)
    }

    pub fn recompute_economy(&mut self) {
        for location in &mut self.locations {
            location.economy = compute_location_economy(location);
        }

        for player in &mut self.players {
            let connected_owned_locations: Vec<&LocationState> = self
                .locations
                .iter()
                .filter(|location| {
                    location.controller == Some(player.player_id)
                        && location.territory == TerritoryState::Owned
                        && location.economy.connected_to_empire
                })
                .collect();
            let disconnected_owned_location_ids = self
                .locations
                .iter()
                .filter(|location| {
                    location.controller == Some(player.player_id)
                        && location.territory == TerritoryState::Owned
                        && !location.economy.connected_to_empire
                })
                .map(|location| location.location_id)
                .collect();

            player.economy = PlayerEconomyState {
                total_connected_energy: connected_owned_locations
                    .iter()
                    .map(|location| location.economy.generated_energy)
                    .sum(),
                total_connected_datacenter_capacity: connected_owned_locations
                    .iter()
                    .map(|location| location.economy.datacenter_capacity)
                    .sum(),
                usable_throughput: connected_owned_locations
                    .iter()
                    .map(|location| location.economy.empire_usable_throughput)
                    .sum(),
                disconnected_owned_location_ids,
            };
            player.throughput.available = player.economy.usable_throughput;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub player_id: PlayerId,
    pub economy: PlayerEconomyState,
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
            economy: PlayerEconomyState::default(),
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
    pub resource_richness: ResourceRichness,
    pub energy_potential: EnergyPotential,
    pub build_capacity: BuildCapacity,
    pub strategic_position: StrategicPosition,
    pub territory: TerritoryState,
    pub controller: Option<PlayerId>,
    pub homeworld_of: Option<PlayerId>,
    pub relay_status: RelayStatus,
    pub orbital_slots: u8,
    pub has_environmental_hazard: bool,
    pub infrastructure: Vec<InfrastructureState>,
    pub economy: LocationEconomyState,
    pub hostile_remnant: Option<HostileRemnantSeed>,
}

impl From<StartingLocation> for LocationState {
    fn from(location: StartingLocation) -> Self {
        Self {
            location_id: location.location_id,
            name: location.name,
            kind: location.kind,
            resource_richness: location.resource_richness,
            energy_potential: location.energy_potential,
            build_capacity: location.build_capacity,
            strategic_position: location.strategic_position,
            territory: location.territory,
            controller: location.controller,
            homeworld_of: location.homeworld_of,
            relay_status: location.relay_status,
            orbital_slots: location.orbital_slots,
            has_environmental_hazard: location.has_environmental_hazard,
            infrastructure: location
                .starting_infrastructure
                .into_iter()
                .map(InfrastructureState::from)
                .collect(),
            economy: LocationEconomyState::default(),
            hostile_remnant: location.hostile_remnant,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureSeed {
    pub kind: InfrastructureKind,
    pub tier: u8,
    pub starts_online: bool,
    pub starts_damaged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureState {
    pub kind: InfrastructureKind,
    pub tier: u8,
    pub condition: InfrastructureCondition,
}

impl From<InfrastructureSeed> for InfrastructureState {
    fn from(seed: InfrastructureSeed) -> Self {
        let condition = if !seed.starts_online {
            InfrastructureCondition::Offline
        } else if seed.starts_damaged {
            InfrastructureCondition::Degraded
        } else {
            InfrastructureCondition::Operational
        };

        Self {
            kind: seed.kind,
            tier: seed.tier,
            condition,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostileRemnantSeed {
    pub kind: HostileRemnantKind,
    pub threat_level: ThreatLevel,
    pub holds_orbital_defenses: bool,
    pub holds_surface_defenses: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerEconomyState {
    pub total_connected_energy: u32,
    pub total_connected_datacenter_capacity: u32,
    pub usable_throughput: u32,
    pub disconnected_owned_location_ids: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationEconomyState {
    pub generated_energy: u32,
    pub datacenter_capacity: u32,
    pub local_usable_throughput: u32,
    pub empire_usable_throughput: u32,
    pub connected_to_empire: bool,
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
pub enum InfrastructureKind {
    CommandNexus,
    MiningSite,
    EnergyProducer,
    Datacenter,
    RelayUplink,
    ShipyardRing,
    MilitaryWorks,
    GroundDefenseSite,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InfrastructureCondition {
    Operational,
    Degraded,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HostileRemnantKind {
    AutonomousDefenseCluster,
    RogueColony,
    DormantMilitaryRuin,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ThreatLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ResourceRichness {
    Sparse,
    Moderate,
    Rich,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EnergyPotential {
    Low,
    Moderate,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuildCapacity {
    Constrained,
    Standard,
    Expansive,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StrategicPosition {
    Peripheral,
    Balanced,
    Central,
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

fn compute_location_economy(location: &LocationState) -> LocationEconomyState {
    if location.territory != TerritoryState::Owned || location.controller.is_none() {
        return LocationEconomyState::default();
    }

    let generated_energy: u32 = location
        .infrastructure
        .iter()
        .filter(|infrastructure| infrastructure.kind == InfrastructureKind::EnergyProducer)
        .map(|infrastructure| energy_output(location.energy_potential.clone(), infrastructure))
        .sum();
    let datacenter_capacity: u32 = location
        .infrastructure
        .iter()
        .filter(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
        .map(|infrastructure| {
            datacenter_capacity_output(location.build_capacity.clone(), infrastructure)
        })
        .sum();
    let local_usable_throughput = generated_energy.min(datacenter_capacity);
    let connected_to_empire = location.relay_status == RelayStatus::Connected
        && location.infrastructure.iter().any(|infrastructure| {
            infrastructure.kind == InfrastructureKind::RelayUplink
                && infrastructure.condition != InfrastructureCondition::Offline
        });
    let empire_usable_throughput = if connected_to_empire {
        local_usable_throughput
    } else {
        0
    };

    LocationEconomyState {
        generated_energy,
        datacenter_capacity,
        local_usable_throughput,
        empire_usable_throughput,
        connected_to_empire,
    }
}

fn energy_output(potential: EnergyPotential, infrastructure: &InfrastructureState) -> u32 {
    let base_output = 40_u32.saturating_mul(u32::from(infrastructure.tier));
    scaled_output(
        base_output,
        energy_potential_modifier(potential),
        &infrastructure.condition,
    )
}

fn datacenter_capacity_output(
    build_capacity: BuildCapacity,
    infrastructure: &InfrastructureState,
) -> u32 {
    let base_capacity = 40_u32.saturating_mul(u32::from(infrastructure.tier));
    scaled_output(
        base_capacity,
        build_capacity_modifier(build_capacity),
        &infrastructure.condition,
    )
}

fn scaled_output(
    base_value: u32,
    local_modifier_percent: u32,
    condition: &InfrastructureCondition,
) -> u32 {
    let conditioned = base_value
        .saturating_mul(local_modifier_percent)
        .saturating_div(100);
    conditioned
        .saturating_mul(condition_modifier_percent(condition))
        .saturating_div(100)
}

const fn energy_potential_modifier(potential: EnergyPotential) -> u32 {
    match potential {
        EnergyPotential::Low => 50,
        EnergyPotential::Moderate => 100,
        EnergyPotential::High => 150,
    }
}

const fn build_capacity_modifier(build_capacity: BuildCapacity) -> u32 {
    match build_capacity {
        BuildCapacity::Constrained => 75,
        BuildCapacity::Standard => 100,
        BuildCapacity::Expansive => 125,
    }
}

const fn condition_modifier_percent(condition: &InfrastructureCondition) -> u32 {
    match condition {
        InfrastructureCondition::Operational => 100,
        InfrastructureCondition::Degraded => 50,
        InfrastructureCondition::Offline => 0,
    }
}
