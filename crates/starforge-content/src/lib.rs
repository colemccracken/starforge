use std::{fmt, fs, path::Path};

use serde::{Deserialize, Serialize};
use starforge_core::{
    GameConfig, LocationKind, MatchSeed, PlayerId, RelayStatus, ScenarioConfig, StartingLocation,
    TerritoryState,
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

pub fn compile_game_config(ruleset: &RulesetDocument) -> GameConfig {
    GameConfig {
        max_players: ruleset.players.max_players,
        ..GameConfig::default()
    }
}

pub fn compile_scenario_config(
    ruleset: &RulesetDocument,
    scenario: &ScenarioDocument,
) -> Result<ScenarioConfig, ContentError> {
    validate_documents(ruleset, scenario)?;

    let starting_locations =
        generate_starting_locations(&ruleset.world, &scenario.players, scenario.seed)?;

    Ok(ScenarioConfig {
        name: scenario.name.clone(),
        player_ids: scenario.players.iter().map(|player| player.id).collect(),
        seed: scenario.seed,
        starting_locations,
    })
}

pub fn default_game_config() -> GameConfig {
    GameConfig::default()
}

pub fn starter_scenario() -> ScenarioConfig {
    ScenarioConfig::default()
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
        locations.push(StartingLocation {
            location_id: u32::try_from(index + 1).expect("player homeworld ids fit in u32"),
            name: player
                .homeworld_name
                .clone()
                .unwrap_or_else(|| format!("Player {} Prime", player.id.0)),
            kind: LocationKind::HabitablePlanet,
            territory: TerritoryState::Owned,
            controller: Some(player.id),
            homeworld_of: Some(player.id),
            relay_status: RelayStatus::Connected,
            orbital_slots: world.homeworld_orbital_slots,
            has_environmental_hazard: false,
            hostile_remnant_present: false,
        });
    }

    for index in players.len()..body_count {
        let kind = roll_location_kind(&mut rng);
        locations.push(StartingLocation {
            location_id: u32::try_from(index + 1).expect("generated location ids fit in u32"),
            name: neutral_world_name(kind.clone(), index + 1),
            kind,
            territory: TerritoryState::Neutral,
            controller: None,
            homeworld_of: None,
            relay_status: RelayStatus::Disconnected,
            orbital_slots: rng.roll_inclusive_u8(
                world.neutral_orbital_slots_min,
                world.neutral_orbital_slots_max,
            ),
            has_environmental_hazard: false,
            hostile_remnant_present: false,
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
        locations[index].hostile_remnant_present = true;
    }

    Ok(locations)
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
        ContentError, compile_scenario_config, parse_ruleset_document, parse_scenario_document,
    };
    use starforge_core::{LocationKind, RelayStatus, TerritoryState};

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
            compile_scenario_config(&ruleset, &scenario).expect("scenario should compile");

        assert_eq!(compiled.player_ids.len(), 2);
        assert!((18..=24).contains(&compiled.starting_locations.len()));

        let homeworlds: Vec<_> = compiled
            .starting_locations
            .iter()
            .filter(|location| location.homeworld_of.is_some())
            .collect();
        assert_eq!(homeworlds.len(), 2);
        assert!(homeworlds.iter().all(|location| {
            location.kind == LocationKind::HabitablePlanet
                && location.territory == TerritoryState::Owned
                && location.controller == location.homeworld_of
                && location.relay_status == RelayStatus::Connected
                && !location.has_environmental_hazard
                && !location.hostile_remnant_present
        }));

        assert!(
            compiled
                .starting_locations
                .iter()
                .any(|location| location.territory == TerritoryState::Neutral)
        );
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
