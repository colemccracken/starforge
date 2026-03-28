pub mod balance;
pub mod command;
pub mod config;
pub mod event;
pub mod ids;
pub mod replay;
pub mod session;
pub mod snapshot;
pub mod state;

pub use balance::{
    ProjectPreview, ResearchPreview, TrainingPreview, buildable_infrastructure_kinds,
    construction_preview, is_unique_infrastructure, repair_preview, research_preview,
    strategic_strike_cost, training_preview,
};
pub use command::{CommandDiscriminant, CommandEnvelope, CommandKind, ValidationError};
pub use config::{GameConfig, LocationConnection, ScenarioConfig, StartingLocation};
pub use event::{EventDiscriminant, EventKind, EventRecord, IndexedEventRecord};
pub use ids::{MatchSeed, PlayerId, SessionId, TickId};
pub use replay::ReplayLog;
pub use session::GameSession;
pub use snapshot::Snapshot;
pub use state::{
    AgentAssignment, BuildCapacity, CommandCollapseState, EnergyPotential, GameState,
    HostileRemnantKind, HostileRemnantSeed, InfrastructureCondition, InfrastructureKind,
    InfrastructureProjectKind, InfrastructureProjectState, InfrastructureSeed, InfrastructureState,
    LocationEconomyState, LocationKind, LocationState, LocationView, LocationVisibility,
    PlayerEconomyState, PlayerResearchState, PlayerState, PlayerStateView, RelayStatus,
    ResearchBranch, ResearchProjectState, ResourceRichness, ResourceStockpiles, StrategicPosition,
    TerritoryState, ThreatLevel, ThroughputBudget, TrainingRunState, TransitKind, TransitState,
    TransitView, VictoryState, VisibilityState,
};

#[cfg(test)]
mod tests {
    use crate::{
        BuildCapacity, CommandCollapseState, CommandEnvelope, CommandKind, EnergyPotential,
        EventKind, GameConfig, GameSession, HostileRemnantKind, HostileRemnantSeed,
        InfrastructureCondition, InfrastructureKind, InfrastructureSeed, LocationConnection,
        LocationKind, LocationVisibility, MatchSeed, PlayerId, RelayStatus, ResearchBranch,
        ResourceRichness, ResourceStockpiles, ScenarioConfig, SessionId, StartingLocation,
        StrategicPosition, TerritoryState, ThreatLevel, TickId, TransitKind, VictoryState,
    };

    fn infrastructure_seed(kind: InfrastructureKind) -> InfrastructureSeed {
        InfrastructureSeed {
            kind,
            tier: 1,
            starts_online: true,
            starts_damaged: false,
        }
    }

    fn damaged_infrastructure_seed(kind: InfrastructureKind) -> InfrastructureSeed {
        InfrastructureSeed {
            starts_damaged: true,
            ..infrastructure_seed(kind)
        }
    }

    fn compute_homeworld(
        player_id: PlayerId,
        location_id: u32,
        name: &str,
        energy_potential: EnergyPotential,
        build_capacity: BuildCapacity,
    ) -> StartingLocation {
        StartingLocation {
            location_id,
            name: name.to_owned(),
            kind: LocationKind::HabitablePlanet,
            resource_richness: ResourceRichness::Rich,
            energy_potential,
            build_capacity,
            strategic_position: StrategicPosition::Balanced,
            territory: TerritoryState::Owned,
            controller: Some(player_id),
            homeworld_of: Some(player_id),
            relay_status: RelayStatus::Connected,
            orbital_slots: 3,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::CommandNexus),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::Datacenter),
                infrastructure_seed(InfrastructureKind::RelayUplink),
                infrastructure_seed(InfrastructureKind::MilitaryWorks),
            ],
            hostile_remnant: None,
        }
    }

    fn economy_fixture_scenario() -> ScenarioConfig {
        ScenarioConfig {
            starting_locations: vec![compute_homeworld(
                PlayerId::new(1),
                1,
                "Helios",
                EnergyPotential::High,
                BuildCapacity::Expansive,
            )],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn power_limited_scenario() -> ScenarioConfig {
        ScenarioConfig {
            starting_locations: vec![compute_homeworld(
                PlayerId::new(1),
                1,
                "Helios",
                EnergyPotential::Low,
                BuildCapacity::Expansive,
            )],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn mining_fixture_scenario() -> ScenarioConfig {
        let mut homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        homeworld
            .starting_infrastructure
            .push(infrastructure_seed(InfrastructureKind::MiningSite));

        ScenarioConfig {
            starting_locations: vec![homeworld],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn hazardous_homeworld_scenario() -> ScenarioConfig {
        let mut homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        homeworld.has_environmental_hazard = true;

        ScenarioConfig {
            starting_locations: vec![homeworld],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn degraded_datacenter_scenario() -> ScenarioConfig {
        let mut homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        if let Some(datacenter) = homeworld
            .starting_infrastructure
            .iter_mut()
            .find(|seed| seed.kind == InfrastructureKind::Datacenter)
        {
            datacenter.starts_damaged = true;
        }

        ScenarioConfig {
            starting_locations: vec![homeworld],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn duplicated_damaged_datacenter_scenario() -> ScenarioConfig {
        let mut homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        if let Some(datacenter) = homeworld
            .starting_infrastructure
            .iter_mut()
            .find(|seed| seed.kind == InfrastructureKind::Datacenter)
        {
            datacenter.starts_damaged = true;
        }
        homeworld
            .starting_infrastructure
            .push(damaged_infrastructure_seed(InfrastructureKind::Datacenter));

        ScenarioConfig {
            starting_locations: vec![homeworld],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn connected_remote_repair_scenario() -> ScenarioConfig {
        let homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let remote = StartingLocation {
            location_id: 2,
            name: "Outpost".to_owned(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Sparse,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(1)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 1,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::RelayUplink),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                damaged_infrastructure_seed(InfrastructureKind::Datacenter),
            ],
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![homeworld, remote],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 20,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn connected_remote_construction_scenario() -> ScenarioConfig {
        let homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let remote = StartingLocation {
            location_id: 2,
            name: "Outpost".to_owned(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Sparse,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(1)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 1,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::RelayUplink),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
            ],
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![homeworld, remote],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 20,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn survey_fixture_scenario() -> ScenarioConfig {
        let homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let neutral = StartingLocation {
            location_id: 2,
            name: "Survey Target".to_owned(),
            kind: LocationKind::Moon,
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Neutral,
            controller: None,
            homeworld_of: None,
            relay_status: RelayStatus::Disconnected,
            orbital_slots: 1,
            has_environmental_hazard: true,
            starting_infrastructure: Vec::new(),
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![homeworld, neutral],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 12,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn long_range_survey_fixture_scenario() -> ScenarioConfig {
        let homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let midpoint = StartingLocation {
            location_id: 2,
            name: "Waypoint".to_owned(),
            kind: LocationKind::Moon,
            resource_richness: ResourceRichness::Sparse,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Neutral,
            controller: None,
            homeworld_of: None,
            relay_status: RelayStatus::Disconnected,
            orbital_slots: 1,
            has_environmental_hazard: false,
            starting_infrastructure: Vec::new(),
            hostile_remnant: None,
        };
        let remote = StartingLocation {
            location_id: 3,
            name: "Far Horizon".to_owned(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::Low,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Neutral,
            controller: None,
            homeworld_of: None,
            relay_status: RelayStatus::Disconnected,
            orbital_slots: 2,
            has_environmental_hazard: true,
            starting_infrastructure: Vec::new(),
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![homeworld, midpoint, remote],
            connections: vec![
                LocationConnection {
                    from_location_id: 1,
                    to_location_id: 2,
                    travel_time_ticks: 7,
                },
                LocationConnection {
                    from_location_id: 2,
                    to_location_id: 3,
                    travel_time_ticks: 11,
                },
            ],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn assault_fixture_scenario() -> ScenarioConfig {
        ScenarioConfig {
            starting_locations: vec![
                compute_homeworld(
                    PlayerId::new(1),
                    1,
                    "Helios",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
                compute_homeworld(
                    PlayerId::new(2),
                    2,
                    "Selene",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
            ],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 10,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn assault_with_defender_colony_scenario() -> ScenarioConfig {
        let defender_colony = StartingLocation {
            location_id: 3,
            name: "Bastion".to_owned(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(2)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 2,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::CommandNexus),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::Datacenter),
                infrastructure_seed(InfrastructureKind::RelayUplink),
            ],
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![
                compute_homeworld(
                    PlayerId::new(1),
                    1,
                    "Helios",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
                compute_homeworld(
                    PlayerId::new(2),
                    2,
                    "Selene",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
                defender_colony,
            ],
            connections: vec![
                LocationConnection {
                    from_location_id: 1,
                    to_location_id: 2,
                    travel_time_ticks: 10,
                },
                LocationConnection {
                    from_location_id: 2,
                    to_location_id: 3,
                    travel_time_ticks: 10,
                },
            ],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn contested_major_visibility_fixture_scenario() -> ScenarioConfig {
        let mut defender_homeworld = compute_homeworld(
            PlayerId::new(2),
            2,
            "Selene",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        defender_homeworld
            .starting_infrastructure
            .push(infrastructure_seed(InfrastructureKind::MiningSite));

        ScenarioConfig {
            starting_locations: vec![
                compute_homeworld(
                    PlayerId::new(1),
                    1,
                    "Helios",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
                defender_homeworld,
            ],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 1,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn undefended_enemy_world_scenario() -> ScenarioConfig {
        let mut enemy_world = compute_homeworld(
            PlayerId::new(2),
            2,
            "Selene",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        enemy_world
            .starting_infrastructure
            .retain(|seed| seed.kind != InfrastructureKind::GroundDefenseSite);

        ScenarioConfig {
            starting_locations: vec![
                compute_homeworld(
                    PlayerId::new(1),
                    1,
                    "Helios",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
                enemy_world,
            ],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 10,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn defended_enemy_world_scenario() -> ScenarioConfig {
        let mut enemy_world = compute_homeworld(
            PlayerId::new(2),
            2,
            "Selene",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        enemy_world
            .starting_infrastructure
            .push(infrastructure_seed(InfrastructureKind::GroundDefenseSite));

        ScenarioConfig {
            starting_locations: vec![
                compute_homeworld(
                    PlayerId::new(1),
                    1,
                    "Helios",
                    EnergyPotential::High,
                    BuildCapacity::Expansive,
                ),
                enemy_world,
            ],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 10,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn claim_fixture_scenario() -> ScenarioConfig {
        let mut scenario = survey_fixture_scenario();
        scenario.starting_locations[1].has_environmental_hazard = false;
        scenario
    }

    fn remnant_fixture_scenario() -> ScenarioConfig {
        let mut scenario = survey_fixture_scenario();
        scenario.starting_locations[1].hostile_remnant = Some(HostileRemnantSeed {
            kind: HostileRemnantKind::AutonomousDefenseCluster,
            threat_level: ThreatLevel::Low,
            holds_orbital_defenses: false,
            holds_surface_defenses: true,
        });
        scenario
    }

    fn ascension_fixture_scenario() -> ScenarioConfig {
        let homeworld = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let second_world = StartingLocation {
            location_id: 2,
            name: "Argent".to_owned(),
            kind: LocationKind::HabitablePlanet,
            resource_richness: ResourceRichness::Rich,
            energy_potential: EnergyPotential::High,
            build_capacity: BuildCapacity::Expansive,
            strategic_position: StrategicPosition::Balanced,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(1)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 2,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::MiningSite),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::Datacenter),
                infrastructure_seed(InfrastructureKind::RelayUplink),
            ],
            hostile_remnant: None,
        };
        let third_world = StartingLocation {
            location_id: 3,
            name: "Cinder".to_owned(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Central,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(1)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 2,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::MiningSite),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::Datacenter),
                infrastructure_seed(InfrastructureKind::RelayUplink),
            ],
            hostile_remnant: None,
        };
        let fourth_world = StartingLocation {
            location_id: 4,
            name: "Nadir".to_owned(),
            kind: LocationKind::Moon,
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(1)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 1,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::MiningSite),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::Datacenter),
                infrastructure_seed(InfrastructureKind::RelayUplink),
            ],
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![homeworld, second_world, third_world, fourth_world],
            connections: vec![
                LocationConnection {
                    from_location_id: 1,
                    to_location_id: 2,
                    travel_time_ticks: 8,
                },
                LocationConnection {
                    from_location_id: 1,
                    to_location_id: 3,
                    travel_time_ticks: 8,
                },
                LocationConnection {
                    from_location_id: 1,
                    to_location_id: 4,
                    travel_time_ticks: 8,
                },
            ],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn command_collapse_fixture_scenario() -> ScenarioConfig {
        let homeworld_one = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let mut homeworld_two = compute_homeworld(
            PlayerId::new(2),
            2,
            "Selene",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        if let Some(command_nexus) = homeworld_two
            .starting_infrastructure
            .iter_mut()
            .find(|seed| seed.kind == InfrastructureKind::CommandNexus)
        {
            command_nexus.starts_online = false;
            command_nexus.starts_damaged = true;
        }

        let remote_colony = StartingLocation {
            location_id: 3,
            name: "Outland".to_owned(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Moderate,
            energy_potential: EnergyPotential::Moderate,
            build_capacity: BuildCapacity::Standard,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Owned,
            controller: Some(PlayerId::new(2)),
            homeworld_of: None,
            relay_status: RelayStatus::Connected,
            orbital_slots: 1,
            has_environmental_hazard: false,
            starting_infrastructure: vec![
                infrastructure_seed(InfrastructureKind::MiningSite),
                infrastructure_seed(InfrastructureKind::EnergyProducer),
                infrastructure_seed(InfrastructureKind::RelayUplink),
            ],
            hostile_remnant: None,
        };

        ScenarioConfig {
            starting_locations: vec![homeworld_one, homeworld_two, remote_colony],
            connections: vec![
                LocationConnection {
                    from_location_id: 2,
                    to_location_id: 3,
                    travel_time_ticks: 6,
                },
                LocationConnection {
                    from_location_id: 1,
                    to_location_id: 2,
                    travel_time_ticks: 10,
                },
            ],
            ..ScenarioConfig::test_fixture()
        }
    }

    fn simultaneous_collapse_fixture_scenario() -> ScenarioConfig {
        let mut homeworld_one = compute_homeworld(
            PlayerId::new(1),
            1,
            "Helios",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        let mut homeworld_two = compute_homeworld(
            PlayerId::new(2),
            2,
            "Selene",
            EnergyPotential::High,
            BuildCapacity::Expansive,
        );
        for homeworld in [&mut homeworld_one, &mut homeworld_two] {
            if let Some(command_nexus) = homeworld
                .starting_infrastructure
                .iter_mut()
                .find(|seed| seed.kind == InfrastructureKind::CommandNexus)
            {
                command_nexus.starts_online = false;
                command_nexus.starts_damaged = true;
            }
        }

        ScenarioConfig {
            starting_locations: vec![homeworld_one, homeworld_two],
            connections: vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 10,
            }],
            ..ScenarioConfig::test_fixture()
        }
    }

    #[test]
    fn new_session_starts_at_tick_zero() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        assert_eq!(session.state().tick_id, TickId::default());
    }

    #[test]
    fn session_bootstraps_from_scenario_starting_locations() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig {
                starting_locations: vec![
                    compute_homeworld(
                        PlayerId::new(1),
                        1,
                        "Helios",
                        EnergyPotential::High,
                        BuildCapacity::Expansive,
                    ),
                    compute_homeworld(
                        PlayerId::new(2),
                        2,
                        "Selene",
                        EnergyPotential::High,
                        BuildCapacity::Expansive,
                    ),
                ],
                connections: vec![LocationConnection {
                    from_location_id: 1,
                    to_location_id: 2,
                    travel_time_ticks: 45,
                }],
                ..ScenarioConfig::test_fixture()
            },
        );

        assert_eq!(session.state().locations.len(), 2);
        assert_eq!(session.state().connections.len(), 1);
        assert_eq!(session.state().locations[0].name, "Helios");
        assert_eq!(
            session.state().locations[0].resource_richness,
            ResourceRichness::Rich
        );
        assert_eq!(
            session.state().locations[0].infrastructure[0].kind,
            InfrastructureKind::CommandNexus
        );
        assert_eq!(session.state().players[0].economy.usable_throughput, 50);
        assert_eq!(
            session.state().locations[0].homeworld_of,
            Some(PlayerId::new(1))
        );
    }

    #[test]
    fn advancing_a_tick_updates_state_and_records_an_event() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        session.advance_tick();

        assert_eq!(session.state().tick_id, TickId::new(1));
        assert_eq!(session.event_log().len(), 2);
    }

    #[test]
    fn accepted_commands_are_written_to_the_replay_log() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::default(),
            command: CommandKind::NoOp,
        };

        session
            .accept_command(command)
            .expect("command should be accepted");

        assert_eq!(session.replay_log().accepted_commands.len(), 1);
    }

    #[test]
    fn throughput_budget_command_updates_player_state() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::default(),
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 10,
                reserved_for_research: 0,
                reserved_for_training: 20,
                reserved_for_agents: 5,
            },
        };

        session
            .accept_command(command)
            .expect("throughput command should be accepted");

        let player = &session.state().players[0];
        assert_eq!(player.throughput.available, 50);
        assert_eq!(player.throughput.reserved_for_training, 20);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandApplied {
                command: CommandKind::SetThroughputBudget { .. },
            }
        )));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::ThroughputBudgetSet {
                reserved_for_model_upkeep: 10,
                reserved_for_research: 0,
                reserved_for_training: 20,
                reserved_for_agents: 5,
                available: 50,
            }
        )));
    }

    #[test]
    fn invalid_throughput_budget_is_rejected_deterministically() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::default(),
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 20,
                reserved_for_research: 0,
                reserved_for_training: 20,
                reserved_for_agents: 20,
            },
        };

        session
            .accept_command(command)
            .expect("command should be accepted for deterministic apply-time validation");

        let player = &session.state().players[0];
        assert_eq!(player.throughput.available, 50);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandRejected { error, .. }
                if error.code == "throughput_overallocated"
        )));
    }

    #[test]
    fn agent_assignment_consumes_available_throughput() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 10,
                    reserved_for_research: 0,
                    reserved_for_training: 5,
                    reserved_for_agents: 0,
                },
            })
            .expect("throughput setup should be accepted");

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::AssignAgent {
                    role: "maintenance_overseer".to_owned(),
                    scope: "homeworld".to_owned(),
                    reserved_throughput: 12,
                },
            })
            .expect("agent assignment should be accepted");

        let player = &session.state().players[0];
        assert_eq!(player.agents.len(), 1);
        assert_eq!(player.throughput.reserved_for_agents, 12);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::AgentAssigned {
                reserved_throughput: 12,
                ..
            }
        )));
    }

    #[test]
    fn commands_scheduled_for_future_ticks_apply_when_due() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            power_limited_scenario(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::new(2),
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 8,
                reserved_for_research: 0,
                reserved_for_training: 3,
                reserved_for_agents: 0,
            },
        };

        session
            .accept_command(command)
            .expect("command should be accepted");
        assert_eq!(session.pending_commands().len(), 1);

        session.advance_tick();
        assert_eq!(session.pending_commands().len(), 1);

        session.advance_tick();
        assert_eq!(session.pending_commands().len(), 0);
        let player = &session.state().players[0];
        assert_eq!(player.throughput.available, 20);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandApplied {
                command: CommandKind::SetThroughputBudget { .. },
            }
        )));
    }

    #[test]
    fn commands_cannot_be_scheduled_in_the_past() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );
        session.advance_tick();

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::new(1),
            apply_at_tick: TickId::default(),
            command: CommandKind::NoOp,
        };

        let error = session
            .accept_command(command)
            .expect_err("past commands should be rejected");

        assert_eq!(error.code, "apply_in_past");
    }

    #[test]
    fn snapshot_round_trip_preserves_pending_commands_and_replay_log() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::new(2),
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 7,
                reserved_for_research: 0,
                reserved_for_training: 4,
                reserved_for_agents: 0,
            },
        };

        session
            .accept_command(command)
            .expect("command should be accepted");

        let snapshot = session.snapshot();
        let mut restored = GameSession::from_snapshot(snapshot);

        assert_eq!(restored.replay_log().accepted_commands.len(), 1);
        assert_eq!(restored.pending_commands().len(), 1);
        assert_eq!(restored.state_hash(), session.state_hash());

        restored.advance_tick();
        restored.advance_tick();

        assert_eq!(restored.pending_commands().len(), 0);
        assert_eq!(restored.state().players[0].throughput.available, 50);
        assert!(restored.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandApplied {
                command: CommandKind::SetThroughputBudget { .. },
            }
        )));
    }

    #[test]
    fn replay_log_can_reconstruct_a_session_state() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        let first_command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::new(2),
            command: CommandKind::RegisterLocation {
                location_id: 100,
                name: "Outer Relay".to_owned(),
            },
        };

        session
            .accept_command(first_command)
            .expect("first command should be accepted");
        session.advance_tick();

        let second_command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::new(1),
            apply_at_tick: TickId::new(2),
            command: CommandKind::SetRelayStatus {
                location_id: 100,
                relay_status: RelayStatus::Disconnected,
            },
        };

        session
            .accept_command(second_command)
            .expect("second command should be accepted");
        session.advance_tick();

        let replayed = GameSession::replay_from_log(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
            session.replay_log().clone(),
        )
        .expect("replay should reconstruct the session");

        assert_eq!(replayed.state_hash(), session.state_hash());
        assert_eq!(
            replayed.pending_commands().len(),
            session.pending_commands().len()
        );
        assert_eq!(
            replayed.replay_log().accepted_commands.len(),
            session.replay_log().accepted_commands.len()
        );
        assert_eq!(replayed.state().locations, session.state().locations);
        assert_eq!(replayed.event_log(), session.event_log());
    }

    #[test]
    fn snapshot_json_round_trip_preserves_session_state() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::new(2),
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 11,
                reserved_for_research: 0,
                reserved_for_training: 9,
                reserved_for_agents: 0,
            },
        };

        session
            .accept_command(command)
            .expect("command should be accepted");
        session.advance_tick();

        let json = session
            .snapshot_json()
            .expect("snapshot should serialize to json");
        let restored =
            GameSession::from_snapshot_json(&json).expect("snapshot should deserialize from json");

        assert_eq!(restored.state_hash(), session.state_hash());
        assert_eq!(restored.pending_commands(), session.pending_commands());
        assert_eq!(
            restored.replay_log().accepted_commands,
            session.replay_log().accepted_commands
        );

        let mut advanced = restored;
        advanced.advance_tick();
        advanced.advance_tick();
        assert_eq!(advanced.state().players[0].throughput.available, 50);
    }

    #[test]
    fn save_load_continuation_matches_uninterrupted_execution() {
        let mut uninterrupted = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        uninterrupted
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::new(2),
                command: CommandKind::RegisterLocation {
                    location_id: 21,
                    name: "Relay Bastion".to_owned(),
                },
            })
            .expect("location registration should be accepted");
        uninterrupted
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::new(3),
                command: CommandKind::SetRelayStatus {
                    location_id: 21,
                    relay_status: RelayStatus::Disconnected,
                },
            })
            .expect("relay update should be accepted");

        uninterrupted.advance_tick();

        let snapshot = uninterrupted
            .snapshot_json()
            .expect("snapshot should serialize");
        let mut restored =
            GameSession::from_snapshot_json(&snapshot).expect("snapshot should deserialize");

        uninterrupted.advance_tick();
        uninterrupted.advance_tick();
        restored.advance_tick();
        restored.advance_tick();

        assert_eq!(restored.state_hash(), uninterrupted.state_hash());
        assert_eq!(restored.event_log(), uninterrupted.event_log());
        assert_eq!(
            restored.pending_commands(),
            uninterrupted.pending_commands()
        );
    }

    #[test]
    fn register_location_command_updates_state_and_emits_domain_event() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::RegisterLocation {
                    location_id: 7,
                    name: "Homeworld".to_owned(),
                },
            })
            .expect("location registration should be accepted");

        assert_eq!(session.state().locations.len(), 1);
        assert_eq!(session.state().locations[0].location_id, 7);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::LocationRegistered { location_id, name }
                if *location_id == 7 && name == "Homeworld"
        )));
    }

    #[test]
    fn duplicate_location_registration_is_rejected_deterministically() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        let command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(1),
            issued_at_tick: TickId::default(),
            apply_at_tick: TickId::default(),
            command: CommandKind::RegisterLocation {
                location_id: 7,
                name: "Homeworld".to_owned(),
            },
        };

        session
            .accept_command(command.clone())
            .expect("first registration should succeed");
        session
            .accept_command(command)
            .expect("duplicate registration should still be accepted for apply-time validation");

        assert_eq!(session.state().locations.len(), 1);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandRejected { error, .. }
                if error.code == "duplicate_location"
        )));
    }

    #[test]
    fn relay_status_command_updates_location_state() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::SetRelayStatus {
                    location_id: 1,
                    relay_status: RelayStatus::Disconnected,
                },
            })
            .expect("relay status update should be accepted");

        assert_eq!(
            session.state().locations[0].relay_status,
            RelayStatus::Disconnected
        );
        assert_eq!(session.state().players[0].throughput.available, 0);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::RelayStatusChanged {
                location_id: 1,
                relay_status: RelayStatus::Disconnected,
            }
        )));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::EconomyUpdated {
                player_id,
                usable_throughput,
                ..
            } if *player_id == PlayerId::new(1) && *usable_throughput == 0
        )));
    }

    #[test]
    fn initial_economy_is_derived_from_seeded_infrastructure() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        assert_eq!(
            session.state().players[0].economy.total_connected_energy,
            60
        );
        assert_eq!(
            session.state().players[0]
                .economy
                .total_connected_datacenter_capacity,
            50
        );
        assert_eq!(session.state().players[0].economy.usable_throughput, 50);
        assert_eq!(session.state().players[0].throughput.available, 50);
        assert_eq!(
            session.state().players[0].economy.connected_stockpiles,
            ResourceStockpiles {
                common_materials: 500,
                volatiles: 120,
                rare_materials: 60,
            }
        );
        assert_eq!(
            session.state().locations[0].economy.local_usable_throughput,
            50
        );
    }

    #[test]
    fn power_shortfall_limits_computed_throughput() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            power_limited_scenario(),
        );

        assert_eq!(session.state().locations[0].economy.generated_energy, 20);
        assert_eq!(session.state().locations[0].economy.datacenter_capacity, 50);
        assert_eq!(
            session.state().locations[0].economy.local_usable_throughput,
            20
        );
        assert_eq!(session.state().players[0].throughput.available, 20);
    }

    #[test]
    fn advancing_ticks_accumulates_resource_extraction_into_stockpiles() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            mining_fixture_scenario(),
        );

        assert_eq!(
            session.state().locations[0].economy.extraction_output,
            ResourceStockpiles {
                common_materials: 12,
                volatiles: 3,
                rare_materials: 2,
            }
        );

        session.advance_tick();

        assert_eq!(
            session.state().locations[0].stockpiles,
            ResourceStockpiles {
                common_materials: 512,
                volatiles: 123,
                rare_materials: 62,
            }
        );
        assert_eq!(
            session.state().players[0].economy.connected_stockpiles,
            ResourceStockpiles {
                common_materials: 512,
                volatiles: 123,
                rare_materials: 62,
            }
        );
    }

    #[test]
    fn infrastructure_wear_degrades_and_eventually_offlines_core_economy() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            hazardous_homeworld_scenario(),
        );

        for _ in 0..100 {
            session.advance_tick();
        }

        let energy_producer = session.state().locations[0]
            .infrastructure
            .iter()
            .find(|infrastructure| infrastructure.kind == InfrastructureKind::EnergyProducer)
            .expect("energy producer should be present");
        let datacenter = session.state().locations[0]
            .infrastructure
            .iter()
            .find(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .expect("datacenter should be present");

        assert_eq!(energy_producer.condition, InfrastructureCondition::Degraded);
        assert_eq!(datacenter.condition, InfrastructureCondition::Degraded);
        assert_eq!(
            session.state().players[0].economy.total_connected_energy,
            30
        );
        assert_eq!(
            session.state().players[0]
                .economy
                .total_connected_datacenter_capacity,
            25
        );
        assert_eq!(session.state().players[0].throughput.available, 25);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureConditionChanged {
                location_id: 1,
                kind: InfrastructureKind::EnergyProducer,
                condition: InfrastructureCondition::Degraded,
            }
        )));

        for _ in 0..100 {
            session.advance_tick();
        }

        let energy_producer = session.state().locations[0]
            .infrastructure
            .iter()
            .find(|infrastructure| infrastructure.kind == InfrastructureKind::EnergyProducer)
            .expect("energy producer should still be present");
        let datacenter = session.state().locations[0]
            .infrastructure
            .iter()
            .find(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .expect("datacenter should still be present");

        assert_eq!(energy_producer.condition, InfrastructureCondition::Offline);
        assert_eq!(datacenter.condition, InfrastructureCondition::Offline);
        assert_eq!(session.state().players[0].economy.usable_throughput, 0);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureConditionChanged {
                location_id: 1,
                kind: InfrastructureKind::EnergyProducer,
                condition: InfrastructureCondition::Offline,
            }
        )));
    }

    #[test]
    fn repair_projects_spend_stockpiles_and_restore_degraded_capacity() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            degraded_datacenter_scenario(),
        );

        assert_eq!(session.state().players[0].throughput.available, 25);

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::QueueInfrastructureRepair {
                    location_id: 1,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            })
            .expect("repair should be accepted");

        assert_eq!(
            session.state().locations[0].stockpiles,
            ResourceStockpiles {
                common_materials: 460,
                volatiles: 110,
                rare_materials: 56,
            }
        );
        assert_eq!(
            session.state().locations[0].infrastructure_projects.len(),
            1
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureRepairQueued {
                location_id: 1,
                kind: InfrastructureKind::Datacenter,
                duration_ticks: 2,
                cost,
            } if *cost == ResourceStockpiles {
                common_materials: 40,
                volatiles: 10,
                rare_materials: 4,
            }
        )));

        session.advance_tick();
        assert_eq!(session.state().players[0].throughput.available, 25);
        assert_eq!(
            session.state().locations[0].infrastructure_projects.len(),
            1
        );

        session.advance_tick();

        let datacenter = session.state().locations[0]
            .infrastructure
            .iter()
            .find(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .expect("datacenter should be present");
        assert_eq!(datacenter.condition, InfrastructureCondition::Operational);
        assert_eq!(session.state().players[0].throughput.available, 50);
        assert!(
            session.state().locations[0]
                .infrastructure_projects
                .is_empty()
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureRepairCompleted {
                location_id: 1,
                kind: InfrastructureKind::Datacenter,
            }
        )));
    }

    #[test]
    fn construction_projects_spend_stockpiles_and_expand_throughput() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::QueueInfrastructureConstruction {
                    location_id: 1,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            })
            .expect("construction should be accepted");

        assert_eq!(
            session.state().locations[0].stockpiles,
            ResourceStockpiles {
                common_materials: 420,
                volatiles: 100,
                rare_materials: 52,
            }
        );
        assert_eq!(
            session.state().locations[0].infrastructure_projects.len(),
            1
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureConstructionQueued {
                location_id: 1,
                kind: InfrastructureKind::Datacenter,
                duration_ticks: 3,
                cost,
            } if *cost == ResourceStockpiles {
                common_materials: 80,
                volatiles: 20,
                rare_materials: 8,
            }
        )));

        session.advance_tick();
        session.advance_tick();
        assert_eq!(session.state().players[0].throughput.available, 50);

        session.advance_tick();

        let datacenter_count = session.state().locations[0]
            .infrastructure
            .iter()
            .filter(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .count();
        assert_eq!(datacenter_count, 2);
        assert_eq!(session.state().players[0].throughput.available, 60);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureConstructionCompleted {
                location_id: 1,
                kind: InfrastructureKind::Datacenter,
            }
        )));
    }

    #[test]
    fn duplicate_same_kind_repairs_can_be_queued_and_completed() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            duplicated_damaged_datacenter_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::QueueInfrastructureRepair {
                    location_id: 1,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            })
            .expect("first duplicate repair should be accepted");
        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::QueueInfrastructureRepair {
                    location_id: 1,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            })
            .expect("second duplicate repair should be accepted");

        assert_eq!(
            session.state().locations[0].infrastructure_projects.len(),
            2
        );

        session.advance_tick();
        session.advance_tick();

        let repaired_datacenters = session.state().locations[0]
            .infrastructure
            .iter()
            .filter(|infrastructure| {
                infrastructure.kind == InfrastructureKind::Datacenter
                    && infrastructure.condition == InfrastructureCondition::Operational
            })
            .count();

        assert_eq!(repaired_datacenters, 2);
        assert_eq!(session.state().players[0].throughput.available, 60);
        assert_eq!(
            session
                .event_log()
                .iter()
                .filter(|event| {
                    matches!(
                        &event.kind,
                        EventKind::InfrastructureRepairCompleted {
                            location_id: 1,
                            kind: InfrastructureKind::Datacenter,
                        }
                    )
                })
                .count(),
            2
        );
    }

    #[test]
    fn connected_repairs_can_draw_from_empire_stockpiles() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            connected_remote_repair_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::QueueInfrastructureRepair {
                    location_id: 2,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            })
            .expect("connected repair should be accepted");

        assert_eq!(
            session.state().players[0].economy.connected_stockpiles,
            ResourceStockpiles {
                common_materials: 520,
                volatiles: 120,
                rare_materials: 56,
            }
        );
        assert_eq!(
            session.state().locations[1].stockpiles,
            ResourceStockpiles {
                common_materials: 20,
                volatiles: 0,
                rare_materials: 0,
            }
        );
        assert_eq!(
            session.state().locations[0].stockpiles,
            ResourceStockpiles {
                common_materials: 500,
                volatiles: 120,
                rare_materials: 56,
            }
        );
    }

    #[test]
    fn connected_construction_can_draw_from_empire_stockpiles() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            connected_remote_construction_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::QueueInfrastructureConstruction {
                    location_id: 2,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            })
            .expect("connected construction should be accepted");

        assert_eq!(
            session.state().players[0].economy.connected_stockpiles,
            ResourceStockpiles {
                common_materials: 480,
                volatiles: 110,
                rare_materials: 52,
            }
        );
        assert_eq!(
            session.state().locations[1].stockpiles,
            ResourceStockpiles {
                common_materials: 0,
                volatiles: 0,
                rare_materials: 0,
            }
        );
        assert_eq!(
            session.state().locations[0].stockpiles,
            ResourceStockpiles {
                common_materials: 480,
                volatiles: 110,
                rare_materials: 52,
            }
        );

        for _ in 0..4 {
            session.advance_tick();
        }

        let remote_datacenter_count = session.state().locations[1]
            .infrastructure
            .iter()
            .filter(|infrastructure| infrastructure.kind == InfrastructureKind::Datacenter)
            .count();
        assert_eq!(remote_datacenter_count, 1);
        assert_eq!(session.state().players[0].throughput.available, 90);
    }

    #[test]
    fn player_view_shows_owned_worlds_and_hides_unsurveyed_others() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig {
                starting_locations: vec![
                    compute_homeworld(
                        PlayerId::new(1),
                        1,
                        "Helios",
                        EnergyPotential::High,
                        BuildCapacity::Expansive,
                    ),
                    compute_homeworld(
                        PlayerId::new(2),
                        2,
                        "Selene",
                        EnergyPotential::High,
                        BuildCapacity::Expansive,
                    ),
                ],
                ..ScenarioConfig::test_fixture()
            },
        );

        let player_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available");

        let owned = player_view
            .locations
            .iter()
            .find(|location| location.location_id == 1)
            .expect("owned location should be present");
        let hidden = player_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("other location should be present");

        assert_eq!(owned.visibility, LocationVisibility::Owned);
        assert!(owned.infrastructure.is_some());
        assert_eq!(hidden.visibility, LocationVisibility::Obscured);
        assert!(hidden.kind.is_none());
    }

    #[test]
    fn player_view_projects_routes_from_known_worlds() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            survey_fixture_scenario(),
        );

        let player_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available");

        assert_eq!(
            player_view.routes,
            vec![LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 12,
            }]
        );
    }

    #[test]
    fn survey_transit_reveals_location_on_arrival_then_intel_goes_stale() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            survey_fixture_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            })
            .expect("survey transit should be accepted");

        let initial_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available");
        let hidden = initial_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("target location should be present");
        assert_eq!(hidden.visibility, LocationVisibility::Obscured);
        assert_eq!(initial_view.transits.len(), 1);
        assert_eq!(initial_view.transits[0].origin_id, 1);
        assert_eq!(initial_view.transits[0].destination_id, 2);
        assert_eq!(initial_view.transits[0].eta_tick, TickId::new(12));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::TransitDispatched {
                origin_id: 1,
                destination_id: 2,
                eta_tick,
                ..
            } if *eta_tick == TickId::new(12)
        )));

        for _ in 0..12 {
            session.advance_tick();
        }

        let observed_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available");
        let observed = observed_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("surveyed location should be present");

        assert_eq!(observed.visibility, LocationVisibility::Observed);
        assert_eq!(observed.kind, Some(LocationKind::Moon));
        assert!(observed.infrastructure.is_some());
        assert!(observed_view.transits.is_empty());
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::TransitArrived {
                destination_id: 2,
                ..
            }
        )));
        assert!(
            session
                .event_log()
                .iter()
                .any(|event| matches!(&event.kind, EventKind::LocationSurveyed { location_id: 2 }))
        );

        session.advance_tick();

        let stale_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available");
        let stale = stale_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("stale location should be present");

        assert_eq!(stale.visibility, LocationVisibility::Surveyed);
        assert_eq!(stale.territory, TerritoryState::Neutral);
        assert!(stale.kind.is_some());
        assert!(stale.infrastructure.is_none());
        assert!(stale_view.visibility.stale_location_ids.contains(&2));
    }

    #[test]
    fn survey_transit_can_target_distant_world_without_intermediate_surveys() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            long_range_survey_fixture_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 3,
                },
            })
            .expect("long-range survey transit should be accepted");

        let initial_view = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available");
        assert_eq!(initial_view.transits.len(), 1);
        assert_eq!(initial_view.transits[0].origin_id, 1);
        assert_eq!(initial_view.transits[0].destination_id, 3);
        assert_eq!(initial_view.transits[0].eta_tick, TickId::new(18));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::TransitDispatched {
                origin_id: 1,
                destination_id: 3,
                eta_tick,
                kind: TransitKind::Survey,
                ..
            } if *eta_tick == TickId::new(18)
        )));
    }

    #[test]
    fn player_event_feed_includes_own_transit_and_survey_events() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            survey_fixture_scenario(),
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(1),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            })
            .expect("survey transit should be accepted");

        for _ in 0..12 {
            session.advance_tick();
        }

        let events = session
            .player_events(PlayerId::new(1), TickId::default())
            .expect("player event feed should be available");

        assert!(events.iter().any(|event| matches!(
            &event.kind,
            EventKind::TransitDispatched {
                origin_id: 1,
                destination_id: 2,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            EventKind::TransitArrived {
                destination_id: 2,
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(&event.kind, EventKind::LocationSurveyed { location_id: 2 }))
        );
    }

    #[test]
    fn player_event_feed_hides_other_players_private_events() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig {
                starting_locations: vec![
                    compute_homeworld(
                        PlayerId::new(1),
                        1,
                        "Helios",
                        EnergyPotential::High,
                        BuildCapacity::Expansive,
                    ),
                    compute_homeworld(
                        PlayerId::new(2),
                        2,
                        "Selene",
                        EnergyPotential::High,
                        BuildCapacity::Expansive,
                    ),
                ],
                ..ScenarioConfig::test_fixture()
            },
        );

        session
            .accept_command(CommandEnvelope {
                session_id: SessionId::new(1),
                player_id: PlayerId::new(2),
                issued_at_tick: TickId::default(),
                apply_at_tick: TickId::default(),
                command: CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 10,
                    reserved_for_research: 0,
                    reserved_for_training: 5,
                    reserved_for_agents: 0,
                },
            })
            .expect("private command should be accepted");

        let player_one_events = session
            .player_events(PlayerId::new(1), TickId::default())
            .expect("player one event feed should be available");
        let player_two_events = session
            .player_events(PlayerId::new(2), TickId::default())
            .expect("player two event feed should be available");

        assert!(
            !player_one_events
                .iter()
                .any(|event| matches!(&event.kind, EventKind::ThroughputBudgetSet { .. }))
        );
        assert!(
            player_two_events
                .iter()
                .any(|event| matches!(&event.kind, EventKind::ThroughputBudgetSet { .. }))
        );
    }

    #[test]
    fn player_events_from_index_skips_old_entries_without_missing_same_tick_events() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            claim_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(12);

        let indexed_events = session
            .player_events_from_index(PlayerId::new(1), 1)
            .expect("indexed player event feed should be available");

        assert_eq!(
            indexed_events.first().map(|event| event.event_index),
            Some(1)
        );
        assert!(indexed_events.iter().any(|event| matches!(
            &event.record.kind,
            EventKind::TransitArrived {
                destination_id: 2,
                ..
            }
        )));
        assert!(indexed_events.iter().any(|event| matches!(
            &event.record.kind,
            EventKind::LocationSurveyed { location_id: 2 }
        )));
        assert_eq!(
            indexed_events
                .iter()
                .filter(|event| event.record.tick_id == TickId::new(12))
                .count(),
            3
        );

        let replayed = session
            .player_events_from_index(
                PlayerId::new(1),
                indexed_events
                    .last()
                    .expect("expected indexed events")
                    .event_index
                    + 1,
            )
            .expect("follow-up indexed event feed should be available");
        assert!(replayed.is_empty());
    }

    #[test]
    fn claim_transit_turns_surveyed_neutral_world_into_owned_colony() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            claim_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(12);

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchClaimTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("claim transit should be accepted");
        session.advance_ticks(12);

        let claimed = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("claimed location should exist");
        assert_eq!(claimed.territory, TerritoryState::Owned);
        assert_eq!(claimed.controller, Some(PlayerId::new(1)));
        assert!(
            claimed
                .infrastructure
                .iter()
                .any(|infrastructure| infrastructure.kind == InfrastructureKind::CommandNexus)
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::LocationClaimed {
                location_id: 2,
                player_id,
            } if *player_id == PlayerId::new(1)
        )));
    }

    #[test]
    fn pacification_clears_remnants_before_claiming() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            remnant_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(12);

        assert!(
            session
                .issue_command_now(
                    PlayerId::new(1),
                    CommandKind::DispatchClaimTransit {
                        origin_location_id: 1,
                        destination_location_id: 2,
                    },
                )
                .is_err(),
            "claiming with active remnants should fail"
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchPacificationTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("pacification transit should be accepted");
        session.advance_ticks(12);

        let target = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("target location should exist");
        assert!(target.hostile_remnant.is_none());
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::HostileRemnantCleared { location_id: 2 }
        )));
    }

    #[test]
    fn assault_transit_contests_enemy_world_and_reveals_it_to_both_players() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            assault_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(10);

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchAssaultTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("assault transit should be accepted");
        session.advance_ticks(10);

        let location = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("enemy location should exist");
        assert_eq!(location.territory, TerritoryState::Contested);
        assert_eq!(location.controller, Some(PlayerId::new(2)));
        assert_eq!(location.contesting_players, vec![PlayerId::new(1)]);

        let attacker_view = session
            .player_view(PlayerId::new(1))
            .expect("attacker view should be available");
        let defender_view = session
            .player_view(PlayerId::new(2))
            .expect("defender view should be available");
        let attacker_location = attacker_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("attacker should see contested location");
        let defender_location = defender_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("defender should see contested location");

        assert_eq!(attacker_location.visibility, LocationVisibility::Observed);
        assert_eq!(attacker_location.territory, TerritoryState::Contested);
        assert_eq!(attacker_location.controller, Some(PlayerId::new(2)));
        assert_eq!(
            attacker_location.contesting_players,
            Some(vec![PlayerId::new(1)])
        );
        assert_eq!(attacker_location.relay_status, Some(RelayStatus::Connected));
        assert_eq!(attacker_location.economy, None);
        assert_eq!(attacker_location.infrastructure_projects, None);
        assert_eq!(attacker_location.stockpiles, None);
        assert_eq!(defender_location.visibility, LocationVisibility::Observed);
        assert_eq!(defender_location.territory, TerritoryState::Contested);
        assert!(defender_location.relay_status.is_some());
        assert!(defender_location.economy.is_some());
        assert!(defender_location.infrastructure_projects.is_some());

        let defender_events = session
            .player_events(PlayerId::new(2), TickId::default())
            .expect("defender event feed should be available");
        assert!(defender_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::LocationContested {
                location_id: 2,
                attacker_id,
                defender_id,
            } if *attacker_id == PlayerId::new(1) && *defender_id == Some(PlayerId::new(2))
        )));
    }

    #[test]
    fn contested_projection_reveals_major_state_but_hides_local_detail() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            contested_major_visibility_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(1);
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchAssaultTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("assault transit should be accepted");
        session.advance_ticks(1);

        let attacker_view = session
            .player_view(PlayerId::new(1))
            .expect("attacker view should be available");
        let attacker_location = attacker_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("attacker should see contested location");

        let visible_infrastructure = attacker_location
            .infrastructure
            .clone()
            .expect("contested projection should retain major infrastructure");
        assert_eq!(attacker_location.visibility, LocationVisibility::Observed);
        assert_eq!(attacker_location.territory, TerritoryState::Contested);
        assert_eq!(attacker_location.relay_status, Some(RelayStatus::Connected));
        assert_eq!(attacker_location.economy, None);
        assert_eq!(attacker_location.infrastructure_projects, None);
        assert_eq!(attacker_location.stockpiles, None);
        assert!(
            visible_infrastructure
                .iter()
                .any(|infrastructure| { infrastructure.kind == InfrastructureKind::CommandNexus })
        );
        assert!(
            visible_infrastructure
                .iter()
                .any(|infrastructure| { infrastructure.kind == InfrastructureKind::Datacenter })
        );
        assert!(
            visible_infrastructure.iter().any(|infrastructure| {
                infrastructure.kind == InfrastructureKind::EnergyProducer
            })
        );
        assert!(
            visible_infrastructure
                .iter()
                .all(|infrastructure| { infrastructure.kind != InfrastructureKind::MiningSite })
        );
    }

    #[test]
    fn major_structure_completion_is_visible_during_contest_but_minor_completion_is_hidden() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            contested_major_visibility_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(2),
                CommandKind::QueueInfrastructureConstruction {
                    location_id: 2,
                    infrastructure_kind: InfrastructureKind::MiningSite,
                },
            )
            .expect("minor construction should queue");
        session
            .issue_command_now(
                PlayerId::new(2),
                CommandKind::QueueInfrastructureConstruction {
                    location_id: 2,
                    infrastructure_kind: InfrastructureKind::Datacenter,
                },
            )
            .expect("major construction should queue");
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(1);
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchAssaultTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("assault transit should be accepted");
        session.advance_ticks(3);

        let attacker_events = session
            .player_events(PlayerId::new(1), TickId::default())
            .expect("attacker event feed should be available");
        assert!(attacker_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureConstructionCompleted {
                location_id: 2,
                kind,
            } if *kind == InfrastructureKind::Datacenter
        )));
        assert!(!attacker_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::InfrastructureConstructionCompleted {
                location_id: 2,
                kind,
            } if *kind == InfrastructureKind::MiningSite
        )));
    }

    #[test]
    fn contested_world_is_captured_and_recovers_after_pacification() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            assault_with_defender_colony_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(10);

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchAssaultTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("assault transit should be accepted");
        session.advance_ticks(10);
        session.advance_ticks(8);

        let captured_location = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("captured location should exist");
        assert_eq!(captured_location.territory, TerritoryState::Owned);
        assert_eq!(captured_location.controller, Some(PlayerId::new(1)));
        assert!(captured_location.contesting_players.is_empty());
        assert_eq!(captured_location.takeover_attacker, None);
        assert_eq!(captured_location.takeover_ticks_remaining, 0);
        assert_eq!(captured_location.pacification_ticks_remaining, 12);
        assert!(
            captured_location
                .infrastructure
                .iter()
                .all(|infrastructure| infrastructure.condition
                    != InfrastructureCondition::Operational)
        );

        let pacifying_throughput = captured_location.economy.empire_usable_throughput;

        let attacker_view = session
            .player_view(PlayerId::new(1))
            .expect("attacker view should be available");
        let defender_view = session
            .player_view(PlayerId::new(2))
            .expect("defender view should be available");
        let attacker_location = attacker_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("attacker should still see the captured location");
        let defender_location = defender_view
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("defender should still know the captured location exists");

        assert_eq!(attacker_location.visibility, LocationVisibility::Owned);
        assert_eq!(attacker_location.controller, Some(PlayerId::new(1)));
        assert_eq!(attacker_location.pacification_ticks_remaining, Some(12));
        assert_eq!(defender_location.visibility, LocationVisibility::Surveyed);
        assert_eq!(defender_location.territory, TerritoryState::Obscured);
        assert_eq!(session.state().victory, VictoryState::Ongoing);

        let defender_events = session
            .player_events(PlayerId::new(2), TickId::default())
            .expect("defender event feed should be available");
        assert!(defender_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::LocationCaptured {
                location_id: 2,
                attacker_id,
                defender_id,
                pacification_ticks: 12,
            } if *attacker_id == PlayerId::new(1) && *defender_id == PlayerId::new(2)
        )));

        session.advance_ticks(12);

        let recovered_location = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("captured location should still exist");
        assert_eq!(recovered_location.pacification_ticks_remaining, 0);
        assert!(recovered_location.economy.empire_usable_throughput > pacifying_throughput);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::PacificationCompleted {
                location_id: 2,
                player_id,
            } if *player_id == PlayerId::new(1)
        )));
    }

    #[test]
    fn capturing_last_enemy_world_triggers_conquest_victory() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            assault_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(10);

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchAssaultTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("assault transit should be accepted");
        session.advance_ticks(10);
        session.advance_ticks(8);

        assert_eq!(
            session.state().victory,
            VictoryState::Won {
                winner: PlayerId::new(1),
            }
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::VictoryDeclared { winner, reason }
                if *winner == PlayerId::new(1) && reason == "military_conquest"
        )));
    }

    #[test]
    fn strategic_strike_is_intercepted_by_ground_defenses() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            defended_enemy_world_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(10);

        let stockpiles_before = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available")
            .economy
            .connected_stockpiles;

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchStrategicStrike {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("strategic strike should be accepted");

        let stockpiles_after = session
            .player_view(PlayerId::new(1))
            .expect("player view should be available")
            .economy
            .connected_stockpiles;
        assert!(stockpiles_after.common_materials < stockpiles_before.common_materials);
        assert!(stockpiles_after.volatiles < stockpiles_before.volatiles);
        assert!(stockpiles_after.rare_materials < stockpiles_before.rare_materials);

        session.advance_ticks(10);

        let defended_location = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("defended location should exist");
        assert_eq!(defended_location.territory, TerritoryState::Owned);
        assert_eq!(defended_location.controller, Some(PlayerId::new(2)));
        assert_eq!(session.state().victory, VictoryState::Ongoing);

        let attacker_events = session
            .player_events(PlayerId::new(1), TickId::default())
            .expect("attacker event feed should be available");
        let defender_events = session
            .player_events(PlayerId::new(2), TickId::default())
            .expect("defender event feed should be available");
        assert!(attacker_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::StrategicStrikeIntercepted {
                location_id: 2,
                attacker_id,
                defender_id,
            } if *attacker_id == PlayerId::new(1) && *defender_id == PlayerId::new(2)
        )));
        assert!(defender_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::StrategicStrikeIntercepted {
                location_id: 2,
                attacker_id,
                defender_id,
            } if *attacker_id == PlayerId::new(1) && *defender_id == PlayerId::new(2)
        )));
    }

    #[test]
    fn assault_can_be_repelled_by_superior_defense() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            defended_enemy_world_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");

        session.advance_ticks(10);

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchAssaultTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("assault transit should be accepted");

        session.advance_ticks(10);

        let defended_location = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("defended location should exist");
        assert_eq!(defended_location.territory, TerritoryState::Owned);
        assert_eq!(defended_location.controller, Some(PlayerId::new(2)));
        assert_eq!(session.state().victory, VictoryState::Ongoing);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::AssaultRepelled {
                location_id: 2,
                attacker_id,
                defender_id,
            } if *attacker_id == PlayerId::new(1) && *defender_id == PlayerId::new(2)
        )));
    }

    #[test]
    fn strategic_strike_destroys_undefended_enemy_world_and_ends_match() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            undefended_enemy_world_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchSurveyTransit {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("survey transit should be accepted");
        session.advance_ticks(10);

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::DispatchStrategicStrike {
                    origin_location_id: 1,
                    destination_location_id: 2,
                },
            )
            .expect("strategic strike should be accepted");
        session.advance_ticks(10);

        let destroyed_location = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == 2)
            .expect("destroyed location should exist");
        assert_eq!(destroyed_location.territory, TerritoryState::Destroyed);
        assert_eq!(destroyed_location.controller, None);
        assert!(destroyed_location.infrastructure.is_empty());
        assert_eq!(
            session.state().victory,
            VictoryState::Won {
                winner: PlayerId::new(1),
            }
        );

        let attacker_events = session
            .player_events(PlayerId::new(1), TickId::default())
            .expect("attacker event feed should be available");
        let defender_events = session
            .player_events(PlayerId::new(2), TickId::default())
            .expect("defender event feed should be available");
        assert!(attacker_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::LocationDestroyed {
                location_id: 2,
                attacker_id,
                defender_id,
            } if *attacker_id == PlayerId::new(1) && *defender_id == PlayerId::new(2)
        )));
        assert!(defender_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::LocationDestroyed {
                location_id: 2,
                attacker_id,
                defender_id,
            } if *attacker_id == PlayerId::new(1) && *defender_id == PlayerId::new(2)
        )));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::VictoryDeclared { winner, reason }
                if *winner == PlayerId::new(1) && reason == "military_conquest"
        )));
    }

    #[test]
    fn training_runs_advance_model_tier_and_end_in_victory() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ascension_fixture_scenario(),
        );

        for target_tier in 2..=5 {
            let training_requirement = match target_tier {
                2 => 20,
                3 => 35,
                4 => 50,
                5 => 70,
                _ => unreachable!(),
            };

            session
                .issue_command_now(
                    PlayerId::new(1),
                    CommandKind::SetThroughputBudget {
                        reserved_for_model_upkeep: 0,
                        reserved_for_research: 0,
                        reserved_for_training: training_requirement,
                        reserved_for_agents: 0,
                    },
                )
                .expect("budget update should be accepted");
            session
                .issue_command_now(
                    PlayerId::new(1),
                    CommandKind::StartTrainingRun { target_tier },
                )
                .expect("training run should be accepted");

            let duration = match target_tier {
                2 => 32,
                3 => 48,
                4 => 72,
                5 => 96,
                _ => unreachable!(),
            };
            session.advance_ticks(duration);
        }

        let player = session
            .state()
            .players
            .iter()
            .find(|player| player.player_id == PlayerId::new(1))
            .expect("player should exist");
        assert_eq!(player.model_tier, 5);
        assert_eq!(
            session.state().victory,
            crate::VictoryState::Won {
                winner: PlayerId::new(1)
            }
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::VictoryDeclared { winner, .. } if *winner == PlayerId::new(1)
        )));
    }

    #[test]
    fn research_projects_complete_and_unlock_branch_levels() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 0,
                    reserved_for_research: 24,
                    reserved_for_training: 0,
                    reserved_for_agents: 0,
                },
            )
            .expect("research budget should be accepted");
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::StartResearchProject {
                    branch: ResearchBranch::Industry,
                    target_level: 1,
                },
            )
            .expect("research project should be accepted");

        session.advance_ticks(8);

        let player = session
            .player_view(PlayerId::new(1))
            .expect("player view should load");
        assert_eq!(player.research.industry_level, 1);
        assert!(player.research.active_project.is_none());
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::ResearchProjectStarted {
                branch: ResearchBranch::Industry,
                target_level: 1,
                ..
            }
        )));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::ResearchProjectCompleted {
                branch: ResearchBranch::Industry,
                achieved_level: 1,
            }
        )));
    }

    #[test]
    fn research_project_validation_reports_reserved_throughput_shortfall() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            economy_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 0,
                    reserved_for_research: 8,
                    reserved_for_training: 0,
                    reserved_for_agents: 0,
                },
            )
            .expect("research budget should be accepted");
        let error = session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::StartResearchProject {
                    branch: ResearchBranch::Industry,
                    target_level: 1,
                },
            )
            .expect_err("research project should be rejected");

        assert_eq!(error.code, "insufficient_research_budget");
        assert!(error.message.contains("need 16 research throughput"));
        assert!(error.message.contains("only 8 is reserved"));
        assert!(error.message.contains("short 8"));
    }

    #[test]
    fn command_collapse_starts_and_defeats_after_countdown() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            command_collapse_fixture_scenario(),
        );

        session.advance_tick();

        let collapse_view = session
            .player_view(PlayerId::new(2))
            .expect("player view should load");
        assert!(matches!(
            collapse_view.collapse,
            CommandCollapseState::Collapsing { ticks_remaining: 8 }
        ));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandCollapseStarted {
                player_id,
                ticks_remaining,
            } if *player_id == PlayerId::new(2) && *ticks_remaining == 8
        )));

        session.advance_ticks(8);

        let defeated_view = session
            .player_view(PlayerId::new(2))
            .expect("player view should load");
        assert_eq!(defeated_view.collapse, CommandCollapseState::Defeated);
        assert_eq!(
            session.state().victory,
            VictoryState::Won {
                winner: PlayerId::new(1),
            }
        );
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::PlayerDefeated { player_id, reason }
                if *player_id == PlayerId::new(2) && reason == "command_collapse"
        )));
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::VictoryDeclared { winner, reason }
                if *winner == PlayerId::new(1) && reason == "command_collapse"
        )));
        assert!(session.state().locations.iter().all(|location| {
            location.controller != Some(PlayerId::new(2))
                || location.territory == TerritoryState::Destroyed
        }));
    }

    #[test]
    fn repairing_offline_nexus_recovers_from_collapse() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            command_collapse_fixture_scenario(),
        );

        session
            .issue_command_now(
                PlayerId::new(2),
                CommandKind::QueueInfrastructureRepair {
                    location_id: 2,
                    infrastructure_kind: InfrastructureKind::CommandNexus,
                },
            )
            .expect("repair should queue");

        session.advance_ticks(4);

        let player_view = session
            .player_view(PlayerId::new(2))
            .expect("player view should load");
        assert_eq!(player_view.collapse, CommandCollapseState::Stable);
        assert_eq!(session.state().victory, VictoryState::Ongoing);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::CommandCollapseRecovered { player_id } if *player_id == PlayerId::new(2)
        )));
    }

    #[test]
    fn simultaneous_collapse_expiry_resolves_to_single_winner() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            simultaneous_collapse_fixture_scenario(),
        );

        session.advance_ticks(9);

        assert_eq!(
            session.state().victory,
            VictoryState::Won {
                winner: PlayerId::new(2),
            }
        );
        let player_one = session
            .player_view(PlayerId::new(1))
            .expect("player one view should load");
        let player_two = session
            .player_view(PlayerId::new(2))
            .expect("player two view should load");
        assert_eq!(player_one.collapse, CommandCollapseState::Defeated);
        assert!(matches!(
            player_two.collapse,
            CommandCollapseState::Collapsing { .. }
        ));
        assert!(
            session
                .event_log()
                .iter()
                .filter(|event| matches!(&event.kind, EventKind::PlayerDefeated { .. }))
                .count()
                == 1
        );
    }

    #[test]
    fn ascension_sequence_is_interrupted_by_relay_cut() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ascension_fixture_scenario(),
        );

        for (target_tier, reserved_for_training, duration) in
            [(2, 20, 32), (3, 35, 48), (4, 50, 72)]
        {
            session
                .issue_command_now(
                    PlayerId::new(1),
                    CommandKind::SetThroughputBudget {
                        reserved_for_model_upkeep: 0,
                        reserved_for_research: 0,
                        reserved_for_training,
                        reserved_for_agents: 0,
                    },
                )
                .expect("budget update should be accepted");
            session
                .issue_command_now(
                    PlayerId::new(1),
                    CommandKind::StartTrainingRun { target_tier },
                )
                .expect("training run should be accepted");
            session.advance_ticks(duration);
        }

        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 0,
                    reserved_for_research: 0,
                    reserved_for_training: 70,
                    reserved_for_agents: 0,
                },
            )
            .expect("tier five budget should be accepted");
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::StartTrainingRun { target_tier: 5 },
            )
            .expect("tier five training should be accepted");
        session
            .issue_command_now(
                PlayerId::new(1),
                CommandKind::SetRelayStatus {
                    location_id: 1,
                    relay_status: RelayStatus::Disconnected,
                },
            )
            .expect("relay cut should be accepted");

        session.advance_tick();

        let player = session
            .player_view(PlayerId::new(1))
            .expect("player view should load");
        assert!(player.training.is_none());
        assert_eq!(session.state().victory, VictoryState::Ongoing);
        assert!(session.event_log().iter().any(|event| matches!(
            &event.kind,
            EventKind::AscensionInterrupted {
                player_id,
                location_id,
                ..
            } if *player_id == PlayerId::new(1)
                && *location_id == 1
        )));
    }

    #[test]
    fn scenario_locations_can_carry_hostile_remnant_seed_data() {
        let session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig {
                starting_locations: vec![StartingLocation {
                    location_id: 9,
                    name: "Ruin World".to_owned(),
                    kind: LocationKind::BarrenWorld,
                    resource_richness: ResourceRichness::Moderate,
                    energy_potential: EnergyPotential::Low,
                    build_capacity: BuildCapacity::Constrained,
                    strategic_position: StrategicPosition::Peripheral,
                    territory: TerritoryState::Neutral,
                    controller: None,
                    homeworld_of: None,
                    relay_status: RelayStatus::Disconnected,
                    orbital_slots: 1,
                    has_environmental_hazard: true,
                    starting_infrastructure: Vec::new(),
                    hostile_remnant: Some(HostileRemnantSeed {
                        kind: HostileRemnantKind::DormantMilitaryRuin,
                        threat_level: ThreatLevel::Medium,
                        holds_orbital_defenses: true,
                        holds_surface_defenses: true,
                    }),
                }],
                ..ScenarioConfig::test_fixture()
            },
        );

        let remnant = session.state().locations[0]
            .hostile_remnant
            .as_ref()
            .expect("remnant should be present");
        assert_eq!(remnant.kind, HostileRemnantKind::DormantMilitaryRuin);
        assert_eq!(remnant.threat_level, ThreatLevel::Medium);
    }

    #[test]
    fn same_seed_produces_same_random_sequence() {
        let scenario = ScenarioConfig::default();
        let mut session_a =
            GameSession::new(SessionId::new(1), GameConfig::default(), scenario.clone());
        let mut session_b = GameSession::new(SessionId::new(2), GameConfig::default(), scenario);

        let first_a = session_a.next_random_u64();
        let second_a = session_a.next_random_u64();
        let first_b = session_b.next_random_u64();
        let second_b = session_b.next_random_u64();

        assert_eq!(first_a, first_b);
        assert_eq!(second_a, second_b);
    }

    #[test]
    fn different_seeds_produce_different_state_hashes() {
        let session_a = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );
        let session_b = GameSession::new(
            SessionId::new(2),
            GameConfig::default(),
            ScenarioConfig {
                seed: MatchSeed(7),
                ..ScenarioConfig::default()
            },
        );

        assert_ne!(session_a.state_hash(), session_b.state_hash());
    }

    #[test]
    fn snapshot_restore_preserves_rng_sequence() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        let _ = session.next_random_u64();
        let snapshot = session.snapshot_json().expect("snapshot should serialize");
        let mut restored =
            GameSession::from_snapshot_json(&snapshot).expect("snapshot should deserialize");

        assert_eq!(session.next_random_u64(), restored.next_random_u64());
    }

    #[test]
    fn identical_starting_sessions_have_the_same_state_hash() {
        let session_a = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
        );
        let session_b = GameSession::new(
            SessionId::new(2),
            GameConfig::default(),
            ScenarioConfig::default(),
        );

        assert_eq!(session_a.state_hash(), session_b.state_hash());
    }
}
