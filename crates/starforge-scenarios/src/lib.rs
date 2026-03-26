use std::{
    fmt,
    path::{Path, PathBuf},
};

use starforge_content::{ContentError, load_compiled_scenario};
use starforge_core::{GameConfig, GameSession, ScenarioConfig, SessionId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioHarness {
    pub name: String,
    pub ruleset_path: PathBuf,
    pub scenario_path: PathBuf,
    pub game_config: GameConfig,
    pub scenario_config: ScenarioConfig,
}

impl ScenarioHarness {
    pub fn instantiate_session(&self, session_id: SessionId) -> GameSession {
        GameSession::new(
            session_id,
            self.game_config.clone(),
            self.scenario_config.clone(),
        )
    }
}

#[derive(Debug)]
pub enum ScenarioHarnessError {
    Content(ContentError),
}

impl fmt::Display for ScenarioHarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScenarioHarnessError {}

impl From<ContentError> for ScenarioHarnessError {
    fn from(error: ContentError) -> Self {
        Self::Content(error)
    }
}

pub fn load_harness(
    ruleset_path: impl AsRef<Path>,
    scenario_path: impl AsRef<Path>,
) -> Result<ScenarioHarness, ScenarioHarnessError> {
    let ruleset_path = ruleset_path.as_ref().to_path_buf();
    let scenario_path = scenario_path.as_ref().to_path_buf();
    let compiled = load_compiled_scenario(&ruleset_path, &scenario_path)?;

    Ok(ScenarioHarness {
        name: compiled.scenario_config.name.clone(),
        ruleset_path,
        scenario_path,
        game_config: compiled.game_config,
        scenario_config: compiled.scenario_config,
    })
}

pub fn starter_skirmish_harness() -> Result<ScenarioHarness, ScenarioHarnessError> {
    load_harness(default_ruleset_path(), default_scenario_path())
}

pub fn default_ruleset_path() -> PathBuf {
    workspace_root().join("content/ruleset.example.yaml")
}

pub fn default_scenario_path() -> PathBuf {
    workspace_root().join("scenarios/two_player_skirmish.example.yaml")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::{
        ScenarioHarnessError, default_ruleset_path, default_scenario_path, load_harness,
        starter_skirmish_harness,
    };
    use starforge_content::ContentError;
    use starforge_core::{InfrastructureKind, PlayerId, SessionId};

    #[test]
    fn starter_harness_loads_from_repo_files() {
        let harness = starter_skirmish_harness().expect("starter harness should load");

        assert_eq!(harness.name, "two_player_skirmish");
        assert_eq!(harness.game_config.max_players, 2);
        assert_eq!(harness.scenario_config.player_ids.len(), 2);
        assert!((18..=24).contains(&harness.scenario_config.starting_locations.len()));
        assert!(
            harness.scenario_config.connections.len()
                >= harness.scenario_config.starting_locations.len()
        );
        assert!(
            harness
                .scenario_config
                .starting_locations
                .iter()
                .any(|location| {
                    location
                        .starting_infrastructure
                        .iter()
                        .any(|seed| seed.kind == InfrastructureKind::CommandNexus)
                })
        );
        assert!(
            harness
                .scenario_config
                .starting_locations
                .iter()
                .any(|location| location.hostile_remnant.is_some())
        );
    }

    #[test]
    fn harness_can_instantiate_a_session() {
        let harness = starter_skirmish_harness().expect("starter harness should load");
        let session = harness.instantiate_session(SessionId::new(77));

        assert_eq!(session.session_id(), SessionId::new(77));
        assert_eq!(
            session.state().locations.len(),
            harness.scenario_config.starting_locations.len()
        );
        assert_eq!(
            session.state().connections.len(),
            harness.scenario_config.connections.len()
        );
        assert!(
            session
                .state()
                .locations
                .iter()
                .any(|location| location.homeworld_of == Some(PlayerId::new(1)))
        );
    }

    #[test]
    fn load_harness_surfaces_missing_file_errors() {
        let error = load_harness(
            default_ruleset_path(),
            default_scenario_path().with_file_name("missing.example.yaml"),
        )
        .expect_err("missing file should fail");

        assert!(matches!(
            error,
            ScenarioHarnessError::Content(ContentError::Io(_))
        ));
    }
}
