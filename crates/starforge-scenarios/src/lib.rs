use starforge_content::starter_scenario;
use starforge_core::ScenarioConfig;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioHarness {
    pub name: String,
    pub scenario: ScenarioConfig,
}

pub fn starter_skirmish_harness() -> ScenarioHarness {
    ScenarioHarness {
        name: "starter_skirmish".to_owned(),
        scenario: starter_scenario(),
    }
}
