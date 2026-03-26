use serde::{Deserialize, Serialize};

use crate::MatchSeed;
use crate::{LocationKind, PlayerId, RelayStatus, TerritoryState};

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
    pub starting_locations: Vec<StartingLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartingLocation {
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

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            name: "starter_skirmish".to_owned(),
            player_ids: vec![PlayerId::new(1), PlayerId::new(2)],
            seed: MatchSeed(42),
            starting_locations: Vec::new(),
        }
    }
}
