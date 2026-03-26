use std::{fmt, fs, path::Path};

use serde::{Deserialize, Serialize};
use starforge_core::{
    BuildCapacity, EnergyPotential, GameConfig, HostileRemnantKind, HostileRemnantSeed,
    InfrastructureKind, InfrastructureSeed, LocationConnection, LocationKind, MatchSeed, PlayerId,
    RelayStatus, ResourceRichness, ScenarioConfig, StartingLocation, StrategicPosition,
    TerritoryState, ThreatLevel,
};

#[derive(Debug)]
pub enum ContentError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    Validation(String),
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read content file: {error}"),
            Self::Yaml(error) => write!(f, "failed to parse yaml content: {error}"),
            Self::Validation(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ContentError {}

impl From<std::io::Error> for ContentError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_yaml::Error> for ContentError {
    fn from(error: serde_yaml::Error) -> Self {
        Self::Yaml(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledScenario {
    pub game_config: GameConfig,
    pub scenario_config: ScenarioConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesetDocument {
    pub name: String,
    pub version: u32,
    pub players: PlayerRulesetDocument,
    pub world: WorldGenerationDocument,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRulesetDocument {
    pub max_players: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldGenerationDocument {
    pub body_count_min: u8,
    pub body_count_max: u8,
    pub homeworld_orbital_slots: u8,
    pub neutral_orbital_slots_min: u8,
    pub neutral_orbital_slots_max: u8,
    pub hazardous_worlds_min: u8,
    pub hazardous_worlds_max: u8,
    pub hostile_remnant_worlds_min: u8,
    pub hostile_remnant_worlds_max: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioDocument {
    pub name: String,
    pub ruleset: String,
    pub seed: MatchSeed,
    pub players: Vec<PlayerSlotDocument>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSlotDocument {
    pub id: PlayerId,
    pub homeworld_name: Option<String>,
}

pub fn parse_ruleset_document(input: &str) -> Result<RulesetDocument, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

pub fn parse_scenario_document(input: &str) -> Result<ScenarioDocument, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

pub fn load_ruleset_document(path: impl AsRef<Path>) -> Result<RulesetDocument, ContentError> {
    let input = fs::read_to_string(path)?;
    parse_ruleset_document(&input).map_err(ContentError::from)
}

pub fn load_scenario_document(path: impl AsRef<Path>) -> Result<ScenarioDocument, ContentError> {
    let input = fs::read_to_string(path)?;
    parse_scenario_document(&input).map_err(ContentError::from)
}

pub fn load_compiled_scenario(
    ruleset_path: impl AsRef<Path>,
    scenario_path: impl AsRef<Path>,
) -> Result<CompiledScenario, ContentError> {
    let ruleset = load_ruleset_document(ruleset_path)?;
    let scenario = load_scenario_document(scenario_path)?;
    compile_scenario_bundle(&ruleset, &scenario)
}

pub fn compile_game_config(ruleset: &RulesetDocument) -> GameConfig {
    GameConfig {
        max_players: ruleset.players.max_players,
        ..GameConfig::default()
    }
}

pub fn compile_scenario_bundle(
    ruleset: &RulesetDocument,
    scenario: &ScenarioDocument,
) -> Result<CompiledScenario, ContentError> {
    validate_documents(ruleset, scenario)?;

    let starting_locations =
        generate_starting_locations(&ruleset.world, &scenario.players, scenario.seed)?;
    let connections = generate_location_connections(&starting_locations, scenario.seed)?;

    Ok(CompiledScenario {
        game_config: compile_game_config(ruleset),
        scenario_config: ScenarioConfig {
            name: scenario.name.clone(),
            player_ids: scenario.players.iter().map(|player| player.id).collect(),
            seed: scenario.seed,
            starting_locations,
            connections,
        },
    })
}

pub fn compile_scenario_config(
    ruleset: &RulesetDocument,
    scenario: &ScenarioDocument,
) -> Result<ScenarioConfig, ContentError> {
    compile_scenario_bundle(ruleset, scenario).map(|compiled| compiled.scenario_config)
}

fn validate_documents(
    ruleset: &RulesetDocument,
    scenario: &ScenarioDocument,
) -> Result<(), ContentError> {
    if scenario.ruleset != ruleset.name {
        return Err(ContentError::Validation(format!(
            "scenario ruleset '{}' does not match loaded ruleset '{}'",
            scenario.ruleset, ruleset.name
        )));
    }

    if scenario.players.is_empty() {
        return Err(ContentError::Validation(
            "scenario must define at least one player".to_owned(),
        ));
    }

    if scenario.players.len() > usize::from(ruleset.players.max_players) {
        return Err(ContentError::Validation(format!(
            "scenario defines {} players but ruleset supports only {}",
            scenario.players.len(),
            ruleset.players.max_players
        )));
    }

    validate_world_generation(&ruleset.world, scenario.players.len())
}

fn validate_world_generation(
    world: &WorldGenerationDocument,
    player_count: usize,
) -> Result<(), ContentError> {
    if world.body_count_min == 0 {
        return Err(ContentError::Validation(
            "world generation requires at least one body".to_owned(),
        ));
    }

    if world.body_count_min > world.body_count_max {
        return Err(ContentError::Validation(
            "world body_count_min cannot exceed body_count_max".to_owned(),
        ));
    }

    if usize::from(world.body_count_min) < player_count {
        return Err(ContentError::Validation(
            "world body_count_min must leave room for every player homeworld".to_owned(),
        ));
    }

    if world.neutral_orbital_slots_min > world.neutral_orbital_slots_max {
        return Err(ContentError::Validation(
            "neutral_orbital_slots_min cannot exceed neutral_orbital_slots_max".to_owned(),
        ));
    }

    if world.hazardous_worlds_min > world.hazardous_worlds_max {
        return Err(ContentError::Validation(
            "hazardous_worlds_min cannot exceed hazardous_worlds_max".to_owned(),
        ));
    }

    if world.hostile_remnant_worlds_min > world.hostile_remnant_worlds_max {
        return Err(ContentError::Validation(
            "hostile_remnant_worlds_min cannot exceed hostile_remnant_worlds_max".to_owned(),
        ));
    }

    Ok(())
}

fn generate_starting_locations(
    world: &WorldGenerationDocument,
    players: &[PlayerSlotDocument],
    seed: MatchSeed,
) -> Result<Vec<StartingLocation>, ContentError> {
    let mut rng = SeededRng::new(seed.as_u64());
    let body_count = usize::from(rng.roll_inclusive_u8(world.body_count_min, world.body_count_max));

    if body_count < players.len() {
        return Err(ContentError::Validation(
            "generated body count cannot fit all player homeworlds".to_owned(),
        ));
    }

    let mut locations = Vec::with_capacity(body_count);
    for (index, player) in players.iter().enumerate() {
        let attributes = homeworld_attributes();
        locations.push(StartingLocation {
            location_id: u32::try_from(index + 1).expect("player homeworld ids fit in u32"),
            name: player
                .homeworld_name
                .clone()
                .unwrap_or_else(|| format!("Player {} Prime", player.id.0)),
            kind: LocationKind::HabitablePlanet,
            resource_richness: attributes.resource_richness,
            energy_potential: attributes.energy_potential,
            build_capacity: attributes.build_capacity,
            strategic_position: roll_strategic_position(&mut rng, true),
            territory: TerritoryState::Owned,
            controller: Some(player.id),
            homeworld_of: Some(player.id),
            relay_status: RelayStatus::Connected,
            orbital_slots: world.homeworld_orbital_slots,
            has_environmental_hazard: false,
            starting_infrastructure: homeworld_infrastructure_package(),
            hostile_remnant: None,
        });
    }

    for index in players.len()..body_count {
        let kind = roll_location_kind(&mut rng);
        let attributes = roll_world_attributes(&mut rng, &kind);
        locations.push(StartingLocation {
            location_id: u32::try_from(index + 1).expect("generated location ids fit in u32"),
            name: neutral_world_name(kind.clone(), index + 1),
            kind,
            resource_richness: attributes.resource_richness,
            energy_potential: attributes.energy_potential,
            build_capacity: attributes.build_capacity,
            strategic_position: roll_strategic_position(&mut rng, false),
            territory: TerritoryState::Neutral,
            controller: None,
            homeworld_of: None,
            relay_status: RelayStatus::Disconnected,
            orbital_slots: rng.roll_inclusive_u8(
                world.neutral_orbital_slots_min,
                world.neutral_orbital_slots_max,
            ),
            has_environmental_hazard: false,
            starting_infrastructure: Vec::new(),
            hostile_remnant: None,
        });
    }

    let neutral_start = players.len();
    let neutral_count = locations.len().saturating_sub(neutral_start);
    let hazardous_worlds = usize::min(
        neutral_count,
        usize::from(rng.roll_inclusive_u8(world.hazardous_worlds_min, world.hazardous_worlds_max)),
    );
    let hostile_remnant_worlds = usize::min(
        neutral_count,
        usize::from(rng.roll_inclusive_u8(
            world.hostile_remnant_worlds_min,
            world.hostile_remnant_worlds_max,
        )),
    );

    let mut hazard_candidates: Vec<usize> = (neutral_start..locations.len()).collect();
    rng.shuffle(&mut hazard_candidates);
    for &index in hazard_candidates.iter().take(hazardous_worlds) {
        locations[index].has_environmental_hazard = true;
    }

    let mut remnant_candidates: Vec<usize> = (neutral_start..locations.len()).collect();
    rng.shuffle(&mut remnant_candidates);
    for &index in remnant_candidates.iter().take(hostile_remnant_worlds) {
        locations[index].hostile_remnant =
            Some(roll_hostile_remnant(&mut rng, &locations[index].kind));
    }

    validate_generated_locations(&locations, players)?;

    Ok(locations)
}

struct WorldAttributes {
    resource_richness: ResourceRichness,
    energy_potential: EnergyPotential,
    build_capacity: BuildCapacity,
}

fn homeworld_infrastructure_package() -> Vec<InfrastructureSeed> {
    vec![
        infrastructure_seed(InfrastructureKind::CommandNexus),
        infrastructure_seed(InfrastructureKind::MiningSite),
        infrastructure_seed(InfrastructureKind::EnergyProducer),
        infrastructure_seed(InfrastructureKind::Datacenter),
        infrastructure_seed(InfrastructureKind::RelayUplink),
        infrastructure_seed(InfrastructureKind::ShipyardRing),
        infrastructure_seed(InfrastructureKind::MilitaryWorks),
        infrastructure_seed(InfrastructureKind::GroundDefenseSite),
    ]
}

fn infrastructure_seed(kind: InfrastructureKind) -> InfrastructureSeed {
    InfrastructureSeed {
        kind,
        tier: 1,
        starts_online: true,
        starts_damaged: false,
    }
}

fn homeworld_attributes() -> WorldAttributes {
    WorldAttributes {
        resource_richness: ResourceRichness::Rich,
        energy_potential: EnergyPotential::High,
        build_capacity: BuildCapacity::Expansive,
    }
}

fn roll_world_attributes(rng: &mut SeededRng, kind: &LocationKind) -> WorldAttributes {
    match kind {
        LocationKind::HabitablePlanet => WorldAttributes {
            resource_richness: roll_from_slice(
                rng,
                &[ResourceRichness::Moderate, ResourceRichness::Rich],
            ),
            energy_potential: roll_from_slice(
                rng,
                &[EnergyPotential::Moderate, EnergyPotential::High],
            ),
            build_capacity: roll_from_slice(
                rng,
                &[BuildCapacity::Standard, BuildCapacity::Expansive],
            ),
        },
        LocationKind::BarrenWorld => WorldAttributes {
            resource_richness: roll_from_slice(
                rng,
                &[ResourceRichness::Sparse, ResourceRichness::Moderate],
            ),
            energy_potential: roll_from_slice(
                rng,
                &[EnergyPotential::Low, EnergyPotential::Moderate],
            ),
            build_capacity: roll_from_slice(
                rng,
                &[BuildCapacity::Constrained, BuildCapacity::Standard],
            ),
        },
        LocationKind::VolcanicWorld => WorldAttributes {
            resource_richness: roll_from_slice(
                rng,
                &[ResourceRichness::Moderate, ResourceRichness::Rich],
            ),
            energy_potential: EnergyPotential::High,
            build_capacity: roll_from_slice(
                rng,
                &[BuildCapacity::Constrained, BuildCapacity::Standard],
            ),
        },
        LocationKind::IceWorld => WorldAttributes {
            resource_richness: roll_from_slice(
                rng,
                &[ResourceRichness::Sparse, ResourceRichness::Moderate],
            ),
            energy_potential: roll_from_slice(
                rng,
                &[EnergyPotential::Low, EnergyPotential::Moderate],
            ),
            build_capacity: roll_from_slice(
                rng,
                &[BuildCapacity::Constrained, BuildCapacity::Standard],
            ),
        },
        LocationKind::Moon => WorldAttributes {
            resource_richness: roll_from_slice(
                rng,
                &[ResourceRichness::Sparse, ResourceRichness::Moderate],
            ),
            energy_potential: EnergyPotential::Low,
            build_capacity: BuildCapacity::Constrained,
        },
        LocationKind::AsteroidCluster => WorldAttributes {
            resource_richness: ResourceRichness::Rich,
            energy_potential: EnergyPotential::Low,
            build_capacity: BuildCapacity::Constrained,
        },
        LocationKind::GasGiant => WorldAttributes {
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::High,
            build_capacity: BuildCapacity::Constrained,
        },
    }
}

fn roll_strategic_position(rng: &mut SeededRng, is_homeworld: bool) -> StrategicPosition {
    if is_homeworld {
        return roll_from_slice(
            rng,
            &[StrategicPosition::Balanced, StrategicPosition::Central],
        );
    }

    roll_from_slice(
        rng,
        &[
            StrategicPosition::Peripheral,
            StrategicPosition::Balanced,
            StrategicPosition::Central,
        ],
    )
}

fn roll_hostile_remnant(rng: &mut SeededRng, location_kind: &LocationKind) -> HostileRemnantSeed {
    let kind = roll_from_slice(
        rng,
        &[
            HostileRemnantKind::AutonomousDefenseCluster,
            HostileRemnantKind::RogueColony,
            HostileRemnantKind::DormantMilitaryRuin,
        ],
    );

    let threat_level = match location_kind {
        LocationKind::HabitablePlanet | LocationKind::VolcanicWorld => {
            roll_from_slice(rng, &[ThreatLevel::Medium, ThreatLevel::High])
        }
        _ => roll_from_slice(rng, &[ThreatLevel::Low, ThreatLevel::Medium]),
    };

    let holds_orbital_defenses = matches!(
        kind,
        HostileRemnantKind::AutonomousDefenseCluster | HostileRemnantKind::DormantMilitaryRuin
    );
    let holds_surface_defenses = !matches!(
        location_kind,
        LocationKind::AsteroidCluster | LocationKind::GasGiant
    );

    HostileRemnantSeed {
        kind,
        threat_level,
        holds_orbital_defenses,
        holds_surface_defenses,
    }
}

fn validate_generated_locations(
    locations: &[StartingLocation],
    players: &[PlayerSlotDocument],
) -> Result<(), ContentError> {
    let homeworlds: Vec<_> = locations
        .iter()
        .filter(|location| location.homeworld_of.is_some())
        .collect();
    if homeworlds.len() != players.len() {
        return Err(ContentError::Validation(
            "generated locations must contain exactly one homeworld per player".to_owned(),
        ));
    }

    for player in players {
        let matching_homeworlds: Vec<_> = homeworlds
            .iter()
            .filter(|location| location.homeworld_of == Some(player.id))
            .collect();
        if matching_homeworlds.len() != 1 {
            return Err(ContentError::Validation(format!(
                "player {} must have exactly one generated homeworld",
                player.id.0
            )));
        }
    }

    for location in homeworlds {
        if location.kind != LocationKind::HabitablePlanet
            || location.territory != TerritoryState::Owned
            || location.controller != location.homeworld_of
            || location.relay_status != RelayStatus::Connected
            || location.has_environmental_hazard
            || location.hostile_remnant.is_some()
            || location.resource_richness != ResourceRichness::Rich
            || location.energy_potential != EnergyPotential::High
            || location.build_capacity != BuildCapacity::Expansive
            || !has_required_homeworld_infrastructure(location)
        {
            return Err(ContentError::Validation(
                "generated homeworld invariant violated".to_owned(),
            ));
        }
    }

    for location in locations {
        if location.hostile_remnant.is_some() && location.territory != TerritoryState::Neutral {
            return Err(ContentError::Validation(
                "hostile remnants may only appear on neutral locations".to_owned(),
            ));
        }
    }

    Ok(())
}

fn has_required_homeworld_infrastructure(location: &StartingLocation) -> bool {
    let required = [
        InfrastructureKind::CommandNexus,
        InfrastructureKind::MiningSite,
        InfrastructureKind::EnergyProducer,
        InfrastructureKind::Datacenter,
        InfrastructureKind::RelayUplink,
        InfrastructureKind::ShipyardRing,
        InfrastructureKind::MilitaryWorks,
        InfrastructureKind::GroundDefenseSite,
    ];

    required.iter().all(|kind| {
        location.starting_infrastructure.iter().any(|seed| {
            &seed.kind == kind && seed.tier == 1 && seed.starts_online && !seed.starts_damaged
        })
    })
}

fn generate_location_connections(
    locations: &[StartingLocation],
    seed: MatchSeed,
) -> Result<Vec<LocationConnection>, ContentError> {
    if locations.len() < 2 {
        return Ok(Vec::new());
    }

    let mut rng = SeededRng::new(seed.as_u64().wrapping_add(0xa5a5a5a5a5a5a5a5));
    let mut connections = Vec::new();

    for index in 0..locations.len() {
        let next_index = (index + 1) % locations.len();
        connections.push(make_connection(
            &locations[index],
            &locations[next_index],
            &mut rng,
        ));
    }

    let extra_links = usize::max(1, locations.len() / 4);
    for _ in 0..extra_links {
        let from_index = usize::try_from(
            rng.next_u64() % u64::try_from(locations.len()).expect("location len fits"),
        )
        .expect("generated index fits in usize");
        let mut to_index = usize::try_from(
            rng.next_u64() % u64::try_from(locations.len()).expect("location len fits"),
        )
        .expect("generated index fits in usize");

        if from_index == to_index {
            to_index = (to_index + 2) % locations.len();
        }

        let connection = make_connection(&locations[from_index], &locations[to_index], &mut rng);
        if !connections.iter().any(|existing| {
            existing.from_location_id == connection.from_location_id
                && existing.to_location_id == connection.to_location_id
        }) {
            connections.push(connection);
        }
    }

    connections.sort();
    validate_generated_connections(locations, &connections)?;

    Ok(connections)
}

fn make_connection(
    from: &StartingLocation,
    to: &StartingLocation,
    rng: &mut SeededRng,
) -> LocationConnection {
    let (from_location_id, to_location_id) = if from.location_id < to.location_id {
        (from.location_id, to.location_id)
    } else {
        (to.location_id, from.location_id)
    };

    let strategic_bonus = match (&from.strategic_position, &to.strategic_position) {
        (StrategicPosition::Central, StrategicPosition::Central) => 0,
        (StrategicPosition::Central, _) | (_, StrategicPosition::Central) => 5,
        (StrategicPosition::Balanced, StrategicPosition::Balanced) => 10,
        _ => 15,
    };

    LocationConnection {
        from_location_id,
        to_location_id,
        travel_time_ticks: 30 + strategic_bonus + u32::from(rng.roll_inclusive_u8(0, 30)),
    }
}

fn validate_generated_connections(
    locations: &[StartingLocation],
    connections: &[LocationConnection],
) -> Result<(), ContentError> {
    let location_ids: Vec<u32> = locations
        .iter()
        .map(|location| location.location_id)
        .collect();

    for connection in connections {
        if connection.from_location_id >= connection.to_location_id {
            return Err(ContentError::Validation(
                "generated connections must be stored in canonical id order".to_owned(),
            ));
        }

        if connection.travel_time_ticks == 0 {
            return Err(ContentError::Validation(
                "generated connections must have nonzero travel time".to_owned(),
            ));
        }

        if !location_ids.contains(&connection.from_location_id)
            || !location_ids.contains(&connection.to_location_id)
        {
            return Err(ContentError::Validation(
                "generated connection references an unknown location".to_owned(),
            ));
        }
    }

    for location_id in location_ids {
        if !connections.iter().any(|connection| {
            connection.from_location_id == location_id || connection.to_location_id == location_id
        }) {
            return Err(ContentError::Validation(format!(
                "generated location {location_id} is disconnected"
            )));
        }
    }

    Ok(())
}

fn roll_from_slice<T: Clone>(rng: &mut SeededRng, options: &[T]) -> T {
    let index = usize::try_from(rng.next_u64() % u64::try_from(options.len()).expect("fits"))
        .expect("generated index fits in usize");
    options[index].clone()
}

fn roll_location_kind(rng: &mut SeededRng) -> LocationKind {
    match rng.next_u64() % 7 {
        0 => LocationKind::HabitablePlanet,
        1 => LocationKind::BarrenWorld,
        2 => LocationKind::VolcanicWorld,
        3 => LocationKind::IceWorld,
        4 => LocationKind::Moon,
        5 => LocationKind::AsteroidCluster,
        _ => LocationKind::GasGiant,
    }
}

fn neutral_world_name(kind: LocationKind, ordinal: usize) -> String {
    let prefix = match kind {
        LocationKind::HabitablePlanet => "Verdant",
        LocationKind::BarrenWorld => "Barren",
        LocationKind::VolcanicWorld => "Inferno",
        LocationKind::IceWorld => "Glacier",
        LocationKind::Moon => "Moon",
        LocationKind::AsteroidCluster => "Cluster",
        LocationKind::GasGiant => "Giant",
    };

    format!("{prefix} {ordinal}")
}

struct SeededRng {
    state: u64,
}

impl SeededRng {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);

        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);

        z ^ (z >> 31)
    }

    fn roll_inclusive_u8(&mut self, min: u8, max: u8) -> u8 {
        if min == max {
            return min;
        }

        let span = u64::from(max - min) + 1;
        min + u8::try_from(self.next_u64() % span).expect("roll range fits in u8")
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for index in (1..values.len()).rev() {
            let swap_index =
                usize::try_from(self.next_u64() % u64::try_from(index + 1).expect("index fits"))
                    .expect("swap index fits in usize");
            values.swap(index, swap_index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContentError, compile_scenario_bundle, compile_scenario_config, parse_ruleset_document,
        parse_scenario_document,
    };
    use starforge_core::{
        BuildCapacity, EnergyPotential, InfrastructureKind, LocationKind, RelayStatus,
        ResourceRichness, TerritoryState,
    };

    const RULESET_YAML: &str = r#"
name: starter_skirmish
version: 1
players:
  max_players: 2
world:
  body_count_min: 18
  body_count_max: 24
  homeworld_orbital_slots: 3
  neutral_orbital_slots_min: 1
  neutral_orbital_slots_max: 4
  hazardous_worlds_min: 2
  hazardous_worlds_max: 5
  hostile_remnant_worlds_min: 1
  hostile_remnant_worlds_max: 3
"#;

    const SCENARIO_YAML: &str = r#"
name: two_player_skirmish
ruleset: starter_skirmish
seed: 42
players:
  - id: 1
    homeworld_name: Helios
  - id: 2
    homeworld_name: Selene
"#;

    #[test]
    fn parses_typed_ruleset_yaml() {
        let ruleset = parse_ruleset_document(RULESET_YAML).expect("ruleset should parse");

        assert_eq!(ruleset.name, "starter_skirmish");
        assert_eq!(ruleset.players.max_players, 2);
        assert_eq!(ruleset.world.body_count_min, 18);
    }

    #[test]
    fn compile_scenario_config_generates_homeworlds_and_neutral_worlds() {
        let ruleset = parse_ruleset_document(RULESET_YAML).expect("ruleset should parse");
        let scenario = parse_scenario_document(SCENARIO_YAML).expect("scenario should parse");

        let compiled =
            compile_scenario_bundle(&ruleset, &scenario).expect("scenario should compile");
        let scenario_config = compiled.scenario_config;

        assert_eq!(compiled.game_config.max_players, 2);
        assert_eq!(scenario_config.player_ids.len(), 2);
        assert!((18..=24).contains(&scenario_config.starting_locations.len()));
        assert!(scenario_config.connections.len() >= scenario_config.starting_locations.len());

        let homeworlds: Vec<_> = scenario_config
            .starting_locations
            .iter()
            .filter(|location| location.homeworld_of.is_some())
            .collect();
        assert_eq!(homeworlds.len(), 2);
        assert!(homeworlds.iter().all(|location| {
            location.kind == LocationKind::HabitablePlanet
                && location.resource_richness == ResourceRichness::Rich
                && location.energy_potential == EnergyPotential::High
                && location.build_capacity == BuildCapacity::Expansive
                && location.territory == TerritoryState::Owned
                && location.controller == location.homeworld_of
                && location.relay_status == RelayStatus::Connected
                && !location.has_environmental_hazard
                && location.hostile_remnant.is_none()
                && location
                    .starting_infrastructure
                    .iter()
                    .any(|seed| seed.kind == InfrastructureKind::CommandNexus)
        }));

        assert!(
            scenario_config
                .starting_locations
                .iter()
                .any(|location| location.territory == TerritoryState::Neutral)
        );
        assert!(
            scenario_config
                .starting_locations
                .iter()
                .filter(|location| location.hostile_remnant.is_some())
                .all(|location| location.territory == TerritoryState::Neutral)
        );
        assert!(scenario_config.connections.iter().all(|connection| {
            connection.from_location_id < connection.to_location_id
                && connection.travel_time_ticks > 0
        }));
    }

    #[test]
    fn same_seed_generates_same_starting_locations() {
        let ruleset = parse_ruleset_document(RULESET_YAML).expect("ruleset should parse");
        let scenario = parse_scenario_document(SCENARIO_YAML).expect("scenario should parse");

        let first = compile_scenario_config(&ruleset, &scenario).expect("scenario should compile");
        let second = compile_scenario_config(&ruleset, &scenario).expect("scenario should compile");

        assert_eq!(first.starting_locations, second.starting_locations);
    }

    #[test]
    fn different_seeds_generate_different_starting_locations() {
        let ruleset = parse_ruleset_document(RULESET_YAML).expect("ruleset should parse");
        let first_scenario = parse_scenario_document(SCENARIO_YAML).expect("scenario should parse");
        let second_scenario =
            parse_scenario_document(&SCENARIO_YAML.replace("seed: 42", "seed: 99"))
                .expect("scenario should parse");

        let first =
            compile_scenario_config(&ruleset, &first_scenario).expect("scenario should compile");
        let second =
            compile_scenario_config(&ruleset, &second_scenario).expect("scenario should compile");

        assert_ne!(first.starting_locations, second.starting_locations);
    }

    #[test]
    fn compile_rejects_ruleset_name_mismatch() {
        let ruleset = parse_ruleset_document(RULESET_YAML).expect("ruleset should parse");
        let scenario =
            parse_scenario_document(&SCENARIO_YAML.replace("starter_skirmish", "other_ruleset"))
                .expect("scenario should parse");

        let error =
            compile_scenario_config(&ruleset, &scenario).expect_err("compile should reject");

        assert!(matches!(error, ContentError::Validation(_)));
    }
}
