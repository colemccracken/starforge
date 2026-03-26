use serde::{Deserialize, Serialize};

use crate::{MatchSeed, PlayerId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameConfig {
    pub tick_duration_secs: u32,
    pub max_players: u8,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            tick_duration_secs: 1,
            max_players: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioConfig {
    pub name: String,
    pub player_ids: Vec<PlayerId>,
    pub seed: MatchSeed,
}

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            name: "starter_skirmish".to_owned(),
            player_ids: vec![PlayerId::new(1), PlayerId::new(2)],
            seed: MatchSeed(42),
        }
    }
}
