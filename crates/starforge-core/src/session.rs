use crate::{
    CommandEnvelope, EventKind, EventRecord, GameConfig, GameState, ReplayLog, ScenarioConfig,
    SessionId, Snapshot, ValidationError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSession {
    session_id: SessionId,
    config: GameConfig,
    scenario: ScenarioConfig,
    state: GameState,
    event_log: Vec<EventRecord>,
    replay_log: ReplayLog,
}

impl GameSession {
    pub fn new(session_id: SessionId, config: GameConfig, scenario: ScenarioConfig) -> Self {
        let player_ids = scenario.player_ids.clone();
        let event_log = vec![EventRecord {
            tick_id: Default::default(),
            player_id: None,
            kind: EventKind::SessionCreated,
        }];

        Self {
            session_id,
            config,
            scenario,
            state: GameState::new(player_ids),
            event_log,
            replay_log: ReplayLog::default(),
        }
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn config(&self) -> &GameConfig {
        &self.config
    }

    pub const fn scenario(&self) -> &ScenarioConfig {
        &self.scenario
    }

    pub const fn state(&self) -> &GameState {
        &self.state
    }

    pub fn event_log(&self) -> &[EventRecord] {
        &self.event_log
    }

    pub const fn replay_log(&self) -> &ReplayLog {
        &self.replay_log
    }

    pub fn advance_tick(&mut self) {
        self.state.tick_id = self.state.tick_id.next();
        self.event_log.push(EventRecord {
            tick_id: self.state.tick_id,
            player_id: None,
            kind: EventKind::TickAdvanced,
        });
    }

    pub fn accept_command(&mut self, command: CommandEnvelope) -> Result<(), ValidationError> {
        if command.session_id != self.session_id {
            return Err(ValidationError {
                code: "session_mismatch",
                message: "command session does not match the target session".to_owned(),
            });
        }

        self.replay_log.accepted_commands.push(command.clone());
        self.event_log.push(EventRecord {
            tick_id: self.state.tick_id,
            player_id: Some(command.player_id),
            kind: EventKind::CommandAccepted,
        });

        Ok(())
    }

    pub fn state_hash(&self) -> u64 {
        let bytes = serde_json::to_vec(&self.state)
            .expect("authoritative state should serialize for hashing");

        stable_hash(&bytes)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot::new(self.session_id, self.state.clone())
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }

    hash
}
