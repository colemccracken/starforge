use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use starforge_core::{GameConfig, MatchSeed, PlayerId, ScenarioConfig};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesetDocument {
    pub name: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioDocument {
    pub name: String,
    pub seed: MatchSeed,
    pub players: Vec<PlayerId>,
}

pub fn parse_ruleset_document(input: &str) -> Result<Value, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

pub fn default_game_config() -> GameConfig {
    GameConfig::default()
}

pub fn starter_scenario() -> ScenarioConfig {
    ScenarioConfig::default()
}

#[cfg(test)]
mod tests {
    use super::parse_ruleset_document;

    #[test]
    fn parses_placeholder_ruleset_yaml() {
        let yaml = "name: starter_skirmish\nversion: 1\n";
        let document = parse_ruleset_document(yaml).expect("ruleset should parse");

        assert!(document.get("name").is_some());
    }
}
