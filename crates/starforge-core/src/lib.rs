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
        CommandEnvelope, CommandKind, EventKind, GameConfig, GameSession, PlayerId, ScenarioConfig,
        SessionId, TickId,
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
            command: CommandKind::NoOp,
        };

        session
            .accept_command(command)
            .expect("command should be accepted");
        assert_eq!(session.pending_commands().len(), 1);

        session.advance_tick();
        assert_eq!(session.pending_commands().len(), 1);

        session.advance_tick();
        assert_eq!(session.pending_commands().len(), 0);
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
            command: CommandKind::NoOp,
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
            command: CommandKind::NoOp,
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
            command: CommandKind::NoOp,
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
