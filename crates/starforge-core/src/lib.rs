pub mod command;
pub mod config;
pub mod event;
pub mod ids;
pub mod replay;
pub mod session;
pub mod snapshot;
pub mod state;

pub use command::{CommandEnvelope, CommandKind, ValidationError};
pub use config::{GameConfig, ScenarioConfig};
pub use event::{EventKind, EventRecord};
pub use ids::{MatchSeed, PlayerId, SessionId, TickId};
pub use replay::ReplayLog;
pub use session::GameSession;
pub use snapshot::Snapshot;
pub use state::{
    AgentAssignment, CommandCollapseState, GameState, LocationState, PlayerState, RelayStatus,
    ThroughputBudget, TrainingRunState, TransitState, VictoryState, VisibilityState,
};

#[cfg(test)]
mod tests {
    use crate::{
        CommandEnvelope, CommandKind, EventKind, GameConfig, GameSession, MatchSeed, PlayerId,
        ScenarioConfig, SessionId, TickId,
    };

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
            ScenarioConfig::default(),
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
                available: 50,
            },
        };

        session
            .accept_command(command)
            .expect("throughput command should be accepted");

        let player = &session.state().players[0];
        assert_eq!(player.throughput.available, 50);
        assert_eq!(player.throughput.reserved_for_training, 20);
        assert!(
            session
                .event_log()
                .iter()
                .any(|event| event.kind == EventKind::CommandApplied)
        );
    }

    #[test]
    fn invalid_throughput_budget_is_rejected_deterministically() {
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
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 20,
                reserved_for_training: 20,
                reserved_for_agents: 20,
                available: 50,
            },
        };

        session
            .accept_command(command)
            .expect("command should be accepted for deterministic apply-time validation");

        let player = &session.state().players[0];
        assert_eq!(player.throughput.available, 0);
        assert!(
            session
                .event_log()
                .iter()
                .any(|event| event.kind == EventKind::CommandRejected)
        );
    }

    #[test]
    fn agent_assignment_consumes_available_throughput() {
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
                command: CommandKind::SetThroughputBudget {
                    reserved_for_model_upkeep: 10,
                    reserved_for_training: 5,
                    reserved_for_agents: 0,
                    available: 40,
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
    }

    #[test]
    fn commands_scheduled_for_future_ticks_apply_when_due() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
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
                available: 20,
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
        assert!(
            session
                .event_log()
                .iter()
                .any(|event| event.kind == EventKind::CommandApplied)
        );
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
            ScenarioConfig::default(),
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
                available: 25,
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
        assert_eq!(restored.state().players[0].throughput.available, 25);
        assert!(
            restored
                .event_log()
                .iter()
                .any(|event| event.kind == EventKind::CommandApplied)
        );
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
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 9,
                reserved_for_training: 6,
                reserved_for_agents: 0,
                available: 30,
            },
        };

        session
            .accept_command(first_command)
            .expect("first command should be accepted");
        session.advance_tick();

        let second_command = CommandEnvelope {
            session_id: SessionId::new(1),
            player_id: PlayerId::new(2),
            issued_at_tick: TickId::new(1),
            apply_at_tick: TickId::new(1),
            command: CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: 5,
                reserved_for_training: 3,
                reserved_for_agents: 0,
                available: 18,
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
        assert_eq!(
            replayed.state().players[0].throughput,
            session.state().players[0].throughput
        );
        assert_eq!(
            replayed.state().players[1].throughput,
            session.state().players[1].throughput
        );
    }

    #[test]
    fn snapshot_json_round_trip_preserves_session_state() {
        let mut session = GameSession::new(
            SessionId::new(1),
            GameConfig::default(),
            ScenarioConfig::default(),
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
                available: 32,
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
        assert_eq!(advanced.state().players[0].throughput.available, 32);
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
