pub mod command;
pub mod config;
pub mod event;
pub mod ids;
pub mod replay;
pub mod session;
pub mod snapshot;
pub mod state;

pub use command::{CommandEnvelope, CommandKind, ValidationError};
pub use config::{GameConfig, LocationConnection, ScenarioConfig, StartingLocation};
pub use event::{EventKind, EventRecord};
pub use ids::{MatchSeed, PlayerId, SessionId, TickId};
pub use replay::ReplayLog;
pub use session::GameSession;
pub use snapshot::Snapshot;
pub use state::{
    AgentAssignment, BuildCapacity, CommandCollapseState, EnergyPotential, GameState,
    HostileRemnantKind, HostileRemnantSeed, InfrastructureCondition, InfrastructureKind,
    InfrastructureProjectKind, InfrastructureProjectState, InfrastructureSeed, InfrastructureState,
    LocationEconomyState, LocationKind, LocationState, PlayerEconomyState, PlayerState,
    RelayStatus, ResourceRichness, ResourceStockpiles, StrategicPosition, TerritoryState,
    ThreatLevel, ThroughputBudget, TrainingRunState, TransitState, VictoryState, VisibilityState,
};

#[cfg(test)]
mod tests {
    use crate::{
        BuildCapacity, CommandEnvelope, CommandKind, EnergyPotential, EventKind, GameConfig,
        GameSession, HostileRemnantKind, HostileRemnantSeed, InfrastructureCondition,
        InfrastructureKind, InfrastructureSeed, LocationConnection, LocationKind, MatchSeed,
        PlayerId, RelayStatus, ResourceRichness, ResourceStockpiles, ScenarioConfig, SessionId,
        StartingLocation, StrategicPosition, TerritoryState, ThreatLevel, TickId,
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

        for _ in 0..34 {
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

        for _ in 0..33 {
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
