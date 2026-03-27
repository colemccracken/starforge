use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoStaticStr};

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
                connected_stockpiles: connected_owned_locations.iter().fold(
                    ResourceStockpiles::default(),
                    |mut total, location| {
                        total.add_assign(&location.stockpiles);
                        total
                    },
                ),
                disconnected_owned_location_ids,
            };
            player.throughput.available = player.economy.usable_throughput;
        }
    }

    pub fn advance_resource_extraction(&mut self) {
        for location in &mut self.locations {
            if location.territory != TerritoryState::Owned || location.controller.is_none() {
                continue;
            }

            location
                .stockpiles
                .add_assign(&location.economy.extraction_output);
        }

        self.recompute_economy();
    }

    pub(crate) fn next_transit_id(&self) -> u32 {
        self.transits
            .iter()
            .map(|transit| transit.transit_id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub(crate) fn resolve_arrived_transits(&mut self) -> Vec<TransitState> {
        let mut arrived = Vec::new();
        let mut in_flight = Vec::with_capacity(self.transits.len());

        for transit in std::mem::take(&mut self.transits) {
            if transit.eta_tick <= self.tick_id {
                arrived.push(transit);
            } else {
                in_flight.push(transit);
            }
        }

        self.transits = in_flight;
        arrived
    }

    pub(crate) fn advance_infrastructure_projects(
        &mut self,
    ) -> Vec<InfrastructureProjectCompletion> {
        let mut completions = Vec::new();

        for location in &mut self.locations {
            if location.infrastructure_projects.is_empty() {
                continue;
            }

            for project in &mut location.infrastructure_projects {
                if project.remaining_ticks > 0 {
                    project.remaining_ticks -= 1;
                }
            }

            let mut remaining_projects = Vec::with_capacity(location.infrastructure_projects.len());
            for project in std::mem::take(&mut location.infrastructure_projects) {
                if project.remaining_ticks == 0 {
                    match project.kind {
                        InfrastructureProjectKind::Repair {
                            infrastructure_kind,
                            target_index,
                        } => {
                            if let Some(infrastructure) = location
                                .infrastructure
                                .get_mut(target_index)
                                .filter(|infrastructure| infrastructure.kind == infrastructure_kind)
                            {
                                infrastructure.condition = InfrastructureCondition::Operational;
                                infrastructure.wear = 0;
                                completions.push(InfrastructureProjectCompletion {
                                    location_id: location.location_id,
                                    kind: infrastructure_kind.clone(),
                                    project_kind: InfrastructureProjectKind::Repair {
                                        infrastructure_kind,
                                        target_index,
                                    },
                                });
                            }
                        }
                        InfrastructureProjectKind::Construction {
                            infrastructure_kind,
                        } => {
                            location.infrastructure.push(InfrastructureState {
                                kind: infrastructure_kind.clone(),
                                tier: 1,
                                condition: InfrastructureCondition::Operational,
                                wear: 0,
                            });
                            completions.push(InfrastructureProjectCompletion {
                                location_id: location.location_id,
                                kind: infrastructure_kind.clone(),
                                project_kind: InfrastructureProjectKind::Construction {
                                    infrastructure_kind,
                                },
                            });
                        }
                    }
                } else {
                    remaining_projects.push(project);
                }
            }

            location.infrastructure_projects = remaining_projects;
        }

        if !completions.is_empty() {
            self.recompute_economy();
        }

        completions
    }

    pub(crate) fn advance_infrastructure_wear(&mut self) -> Vec<InfrastructureConditionChange> {
        let mut changes = Vec::new();

        for location in &mut self.locations {
            if location.territory != TerritoryState::Owned || location.controller.is_none() {
                continue;
            }

            let has_environmental_hazard = location.has_environmental_hazard;
            for infrastructure in &mut location.infrastructure {
                let previous_condition = infrastructure.condition.clone();
                infrastructure.wear = infrastructure.wear.saturating_add(infrastructure_wear_rate(
                    has_environmental_hazard,
                    &infrastructure.kind,
                ));
                infrastructure.condition = condition_for_wear(infrastructure.wear);

                if infrastructure.condition != previous_condition {
                    changes.push(InfrastructureConditionChange {
                        location_id: location.location_id,
                        kind: infrastructure.kind.clone(),
                        condition: infrastructure.condition.clone(),
                    });
                }
            }
        }

        if !changes.is_empty() {
            self.recompute_economy();
        }

        changes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub player_id: PlayerId,
    pub model_tier: u8,
    pub economy: PlayerEconomyState,
    pub throughput: ThroughputBudget,
    pub research: PlayerResearchState,
    pub visibility: VisibilityState,
    pub training: Option<TrainingRunState>,
    pub collapse: CommandCollapseState,
    pub agents: Vec<AgentAssignment>,
}

impl PlayerState {
    pub fn new(player_id: PlayerId) -> Self {
        Self {
            player_id,
            model_tier: 1,
            economy: PlayerEconomyState::default(),
            throughput: ThroughputBudget::default(),
            research: PlayerResearchState::default(),
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
    pub infrastructure_projects: Vec<InfrastructureProjectState>,
    pub economy: LocationEconomyState,
    pub stockpiles: ResourceStockpiles,
    pub hostile_remnant: Option<HostileRemnantSeed>,
    #[serde(default)]
    pub contesting_players: Vec<PlayerId>,
    #[serde(default)]
    pub takeover_attacker: Option<PlayerId>,
    #[serde(default)]
    pub takeover_ticks_remaining: u32,
    #[serde(default)]
    pub pacification_ticks_remaining: u32,
}

impl From<StartingLocation> for LocationState {
    fn from(location: StartingLocation) -> Self {
        let stockpiles = initial_stockpiles(&location);
        let infrastructure = location
            .starting_infrastructure
            .into_iter()
            .map(InfrastructureState::from)
            .collect();

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
            infrastructure,
            infrastructure_projects: Vec::new(),
            economy: LocationEconomyState::default(),
            stockpiles,
            hostile_remnant: location.hostile_remnant,
            contesting_players: Vec::new(),
            takeover_attacker: None,
            takeover_ticks_remaining: 0,
            pacification_ticks_remaining: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitState {
    pub transit_id: u32,
    pub player_id: PlayerId,
    pub origin_id: u32,
    pub destination_id: u32,
    pub eta_tick: TickId,
    pub kind: TransitKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibilityState {
    pub surveyed_location_ids: Vec<u32>,
    pub observed_location_ids: Vec<u32>,
    pub stale_location_ids: Vec<u32>,
    pub contested_location_ids: Vec<u32>,
}

impl VisibilityState {
    pub fn mark_surveyed(&mut self, location_id: u32) {
        push_unique_sorted(&mut self.surveyed_location_ids, location_id);
        push_unique_sorted(&mut self.observed_location_ids, location_id);
        self.stale_location_ids
            .retain(|known_id| *known_id != location_id);
    }

    pub fn mark_contested(&mut self, location_id: u32) {
        self.mark_surveyed(location_id);
        push_unique_sorted(&mut self.contested_location_ids, location_id);
    }

    pub fn refresh_owned_and_contested(
        &mut self,
        owned_location_ids: &[u32],
        contested_location_ids: &[u32],
    ) {
        let mut observed_location_ids = owned_location_ids.to_vec();
        observed_location_ids.extend(contested_location_ids.iter().copied());
        observed_location_ids.sort_unstable();
        observed_location_ids.dedup();

        for location_id in &observed_location_ids {
            push_unique_sorted(&mut self.surveyed_location_ids, *location_id);
        }

        self.observed_location_ids = observed_location_ids;
        self.contested_location_ids = contested_location_ids.to_vec();
        self.contested_location_ids.sort_unstable();
        self.contested_location_ids.dedup();
        self.stale_location_ids = self
            .surveyed_location_ids
            .iter()
            .copied()
            .filter(|location_id| !self.observed_location_ids.contains(location_id))
            .collect();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStateView {
    pub tick_id: TickId,
    pub player_id: PlayerId,
    pub model_tier: u8,
    pub economy: PlayerEconomyState,
    pub throughput: ThroughputBudget,
    pub research: PlayerResearchState,
    pub training: Option<TrainingRunState>,
    pub collapse: CommandCollapseState,
    pub visibility: VisibilityState,
    pub locations: Vec<LocationView>,
    pub routes: Vec<LocationConnection>,
    pub transits: Vec<TransitView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationView {
    pub location_id: u32,
    pub name: String,
    pub visibility: LocationVisibility,
    pub territory: TerritoryState,
    pub controller: Option<PlayerId>,
    pub contesting_players: Option<Vec<PlayerId>>,
    pub pacification_ticks_remaining: Option<u32>,
    pub kind: Option<LocationKind>,
    pub resource_richness: Option<ResourceRichness>,
    pub energy_potential: Option<EnergyPotential>,
    pub build_capacity: Option<BuildCapacity>,
    pub relay_status: Option<RelayStatus>,
    pub orbital_slots: Option<u8>,
    pub has_environmental_hazard: Option<bool>,
    pub infrastructure: Option<Vec<InfrastructureState>>,
    pub infrastructure_projects: Option<Vec<InfrastructureProjectState>>,
    pub economy: Option<LocationEconomyState>,
    pub stockpiles: Option<ResourceStockpiles>,
    pub hostile_remnant_present: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LocationVisibility {
    Owned,
    Observed,
    Surveyed,
    Obscured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitView {
    pub transit_id: u32,
    pub origin_id: u32,
    pub destination_id: u32,
    pub eta_tick: TickId,
    pub kind: TransitKind,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum TransitKind {
    Survey,
    Pacification,
    Claim,
    Assault,
    StrategicStrike,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThroughputBudget {
    pub reserved_for_model_upkeep: u32,
    pub reserved_for_training: u32,
    pub reserved_for_research: u32,
    pub reserved_for_agents: u32,
    pub available: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingRunState {
    pub target_tier: u8,
    pub progress_ticks: u32,
    pub required_ticks: u32,
    pub required_training_throughput: u32,
    #[serde(default)]
    pub ascension_site_location_id: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerResearchState {
    pub industry_level: u8,
    pub models_level: u8,
    pub warfare_level: u8,
    pub resilience_level: u8,
    pub active_project: Option<ResearchProjectState>,
}

impl PlayerResearchState {
    pub const fn level_for(&self, branch: ResearchBranch) -> u8 {
        match branch {
            ResearchBranch::Industry => self.industry_level,
            ResearchBranch::Models => self.models_level,
            ResearchBranch::Warfare => self.warfare_level,
            ResearchBranch::Resilience => self.resilience_level,
        }
    }

    pub fn set_level(&mut self, branch: ResearchBranch, level: u8) {
        match branch {
            ResearchBranch::Industry => self.industry_level = level,
            ResearchBranch::Models => self.models_level = level,
            ResearchBranch::Warfare => self.warfare_level = level,
            ResearchBranch::Resilience => self.resilience_level = level,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchProjectState {
    pub branch: ResearchBranch,
    pub target_level: u8,
    pub progress_ticks: u32,
    pub required_ticks: u32,
    pub required_research_throughput: u32,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum ResearchBranch {
    Industry,
    Models,
    Warfare,
    Resilience,
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
    pub wear: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureProjectState {
    pub kind: InfrastructureProjectKind,
    pub remaining_ticks: u32,
    pub total_ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InfrastructureProjectKind {
    Repair {
        infrastructure_kind: InfrastructureKind,
        #[serde(default)]
        target_index: usize,
    },
    Construction {
        infrastructure_kind: InfrastructureKind,
    },
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
            wear: initial_wear_for_condition(&condition),
            condition,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InfrastructureConditionChange {
    pub location_id: u32,
    pub kind: InfrastructureKind,
    pub condition: InfrastructureCondition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InfrastructureProjectCompletion {
    pub location_id: u32,
    pub kind: InfrastructureKind,
    pub project_kind: InfrastructureProjectKind,
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
    pub connected_stockpiles: ResourceStockpiles,
    pub disconnected_owned_location_ids: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationEconomyState {
    pub generated_energy: u32,
    pub datacenter_capacity: u32,
    pub local_usable_throughput: u32,
    pub empire_usable_throughput: u32,
    pub extraction_output: ResourceStockpiles,
    pub connected_to_empire: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStockpiles {
    pub common_materials: u32,
    pub volatiles: u32,
    pub rare_materials: u32,
}

impl ResourceStockpiles {
    pub fn add_assign(&mut self, other: &Self) {
        self.common_materials = self.common_materials.saturating_add(other.common_materials);
        self.volatiles = self.volatiles.saturating_add(other.volatiles);
        self.rare_materials = self.rare_materials.saturating_add(other.rare_materials);
    }

    pub const fn can_cover(&self, other: &Self) -> bool {
        self.common_materials >= other.common_materials
            && self.volatiles >= other.volatiles
            && self.rare_materials >= other.rare_materials
    }

    pub const fn is_zero(&self) -> bool {
        self.common_materials == 0 && self.volatiles == 0 && self.rare_materials == 0
    }

    pub fn spend_partial(&mut self, remaining_cost: &mut Self) {
        let common_materials = self.common_materials.min(remaining_cost.common_materials);
        self.common_materials -= common_materials;
        remaining_cost.common_materials -= common_materials;

        let volatiles = self.volatiles.min(remaining_cost.volatiles);
        self.volatiles -= volatiles;
        remaining_cost.volatiles -= volatiles;

        let rare_materials = self.rare_materials.min(remaining_cost.rare_materials);
        self.rare_materials -= rare_materials;
        remaining_cost.rare_materials -= rare_materials;
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum LocationKind {
    HabitablePlanet,
    BarrenWorld,
    VolcanicWorld,
    IceWorld,
    Moon,
    AsteroidCluster,
    GasGiant,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
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

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum InfrastructureCondition {
    Operational,
    Degraded,
    Offline,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum HostileRemnantKind {
    AutonomousDefenseCluster,
    RogueColony,
    DormantMilitaryRuin,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum ThreatLevel {
    Low,
    Medium,
    High,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum ResourceRichness {
    Sparse,
    Moderate,
    Rich,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum EnergyPotential {
    Low,
    Moderate,
    High,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum BuildCapacity {
    Constrained,
    Standard,
    Expansive,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum StrategicPosition {
    Peripheral,
    Balanced,
    Central,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum TerritoryState {
    Neutral,
    Owned,
    Contested,
    Destroyed,
    Obscured,
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
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
    Defeated,
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

    let pacification_modifier_percent = pacification_modifier_percent(location);
    let generated_energy: u32 = location
        .infrastructure
        .iter()
        .filter(|infrastructure| infrastructure.kind == InfrastructureKind::EnergyProducer)
        .map(|infrastructure| energy_output(location.energy_potential.clone(), infrastructure))
        .sum::<u32>()
        .saturating_mul(pacification_modifier_percent)
        .saturating_div(100);
    let datacenter_capacity: u32 = location
        .infrastructure
        .iter()
        .filter(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
        .map(|infrastructure| {
            datacenter_capacity_output(location.build_capacity.clone(), infrastructure)
        })
        .sum::<u32>()
        .saturating_mul(pacification_modifier_percent)
        .saturating_div(100);
    let local_usable_throughput = generated_energy.min(datacenter_capacity);
    let extraction_output = resource_extraction_output(location);
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
        extraction_output,
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

fn resource_extraction_output(location: &LocationState) -> ResourceStockpiles {
    let mut output = location
        .infrastructure
        .iter()
        .filter(|infrastructure| infrastructure.kind == InfrastructureKind::MiningSite)
        .fold(
            ResourceStockpiles::default(),
            |mut total, infrastructure| {
                let tier = u32::from(infrastructure.tier);
                let condition_percent = condition_modifier_percent(&infrastructure.condition);

                total.common_materials = total.common_materials.saturating_add(
                    extraction_common_materials(location.resource_richness.clone())
                        .saturating_mul(tier)
                        .saturating_mul(condition_percent)
                        .saturating_div(100),
                );
                total.volatiles = total.volatiles.saturating_add(
                    extraction_volatiles(location.energy_potential.clone())
                        .saturating_mul(tier)
                        .saturating_mul(condition_percent)
                        .saturating_div(100),
                );
                total.rare_materials = total.rare_materials.saturating_add(
                    extraction_rare_materials(location.resource_richness.clone())
                        .saturating_mul(tier)
                        .saturating_mul(condition_percent)
                        .saturating_div(100),
                );

                total
            },
        );

    if location.has_environmental_hazard {
        output.common_materials /= 2;
        output.volatiles /= 2;
    }

    let pacification_modifier_percent = pacification_modifier_percent(location);
    output.common_materials = output
        .common_materials
        .saturating_mul(pacification_modifier_percent)
        .saturating_div(100);
    output.volatiles = output
        .volatiles
        .saturating_mul(pacification_modifier_percent)
        .saturating_div(100);
    output.rare_materials = output
        .rare_materials
        .saturating_mul(pacification_modifier_percent)
        .saturating_div(100);

    output
}

const fn pacification_modifier_percent(location: &LocationState) -> u32 {
    if location.pacification_ticks_remaining > 0 {
        50
    } else {
        100
    }
}

const fn condition_for_wear(wear: u32) -> InfrastructureCondition {
    if wear >= OFFLINE_WEAR_THRESHOLD {
        InfrastructureCondition::Offline
    } else if wear >= DEGRADED_WEAR_THRESHOLD {
        InfrastructureCondition::Degraded
    } else {
        InfrastructureCondition::Operational
    }
}

pub(crate) const fn initial_wear_for_condition(condition: &InfrastructureCondition) -> u32 {
    match condition {
        InfrastructureCondition::Operational => 0,
        InfrastructureCondition::Degraded => DEGRADED_WEAR_THRESHOLD,
        InfrastructureCondition::Offline => OFFLINE_WEAR_THRESHOLD,
    }
}

fn infrastructure_wear_rate(
    has_environmental_hazard: bool,
    infrastructure_kind: &InfrastructureKind,
) -> u32 {
    let base_rate = match infrastructure_kind {
        InfrastructureKind::CommandNexus => 1,
        InfrastructureKind::MiningSite => 2,
        InfrastructureKind::EnergyProducer => 2,
        InfrastructureKind::Datacenter => 2,
        InfrastructureKind::RelayUplink => 1,
        InfrastructureKind::ShipyardRing => 2,
        InfrastructureKind::MilitaryWorks => 2,
        InfrastructureKind::GroundDefenseSite => 1,
    };

    if has_environmental_hazard {
        base_rate + 1
    } else {
        base_rate
    }
}

const fn extraction_common_materials(resource_richness: ResourceRichness) -> u32 {
    match resource_richness {
        ResourceRichness::Sparse => 4,
        ResourceRichness::Moderate => 8,
        ResourceRichness::Rich => 12,
    }
}

const fn extraction_volatiles(energy_potential: EnergyPotential) -> u32 {
    match energy_potential {
        EnergyPotential::Low => 1,
        EnergyPotential::Moderate => 2,
        EnergyPotential::High => 3,
    }
}

const fn extraction_rare_materials(resource_richness: ResourceRichness) -> u32 {
    match resource_richness {
        ResourceRichness::Sparse => 0,
        ResourceRichness::Moderate => 1,
        ResourceRichness::Rich => 2,
    }
}

fn push_unique_sorted(values: &mut Vec<u32>, value: u32) {
    if !values.contains(&value) {
        values.push(value);
        values.sort_unstable();
    }
}

pub(crate) fn push_unique_sorted_player_id(values: &mut Vec<PlayerId>, value: PlayerId) {
    if !values.contains(&value) {
        values.push(value);
        values.sort_unstable();
    }
}

const DEGRADED_WEAR_THRESHOLD: u32 = 300;
const OFFLINE_WEAR_THRESHOLD: u32 = 600;

fn initial_stockpiles(location: &StartingLocation) -> ResourceStockpiles {
    if location.homeworld_of.is_some() {
        return ResourceStockpiles {
            common_materials: 500,
            volatiles: 120,
            rare_materials: 60,
        };
    }

    match location.resource_richness {
        ResourceRichness::Sparse => ResourceStockpiles {
            common_materials: 60,
            volatiles: 10,
            rare_materials: 0,
        },
        ResourceRichness::Moderate => ResourceStockpiles {
            common_materials: 120,
            volatiles: 20,
            rare_materials: 10,
        },
        ResourceRichness::Rich => ResourceStockpiles {
            common_materials: 180,
            volatiles: 35,
            rare_materials: 20,
        },
    }
}
