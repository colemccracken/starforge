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
        CommandEnvelope, CommandKind, GameConfig, GameSession, PlayerId, ScenarioConfig, SessionId,
        TickId,
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
