use crate::{
    BuildCapacity, CommandEnvelope, CommandKind, EnergyPotential, EventKind, EventRecord,
    GameConfig, GameState, LocationKind, LocationState, PlayerId, RelayStatus, ReplayLog,
    ResourceRichness, ScenarioConfig, SessionId, Snapshot, StrategicPosition, TerritoryState,
    ValidationError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSession {
    session_id: SessionId,
    config: GameConfig,
    scenario: ScenarioConfig,
    state: GameState,
    event_log: Vec<EventRecord>,
    replay_log: ReplayLog,
    pending_commands: Vec<CommandEnvelope>,
}

impl GameSession {
    pub fn new(session_id: SessionId, config: GameConfig, scenario: ScenarioConfig) -> Self {
        let player_ids = scenario.player_ids.clone();
        let seed = scenario.seed;
        let starting_locations = scenario.starting_locations.clone();
        let connections = scenario.connections.clone();
        let event_log = vec![EventRecord {
            tick_id: Default::default(),
            player_id: None,
            kind: EventKind::SessionCreated {
                player_ids: player_ids.clone(),
                seed,
            },
        }];

        Self {
            session_id,
            config,
            scenario,
            state: GameState::new(player_ids, seed, starting_locations, connections),
            event_log,
            replay_log: ReplayLog::default(),
            pending_commands: Vec::new(),
        }
    }

    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        Self {
            session_id: snapshot.session_id,
            config: snapshot.config,
            scenario: snapshot.scenario,
            state: snapshot.state,
            event_log: snapshot.event_log,
            replay_log: snapshot.replay_log,
            pending_commands: snapshot.pending_commands,
        }
    }

    pub fn from_snapshot_json(json: &str) -> Result<Self, serde_json::Error> {
        Snapshot::from_json(json).map(Self::from_snapshot)
    }

    pub fn replay_from_log(
        session_id: SessionId,
        config: GameConfig,
        scenario: ScenarioConfig,
        replay_log: ReplayLog,
    ) -> Result<Self, ValidationError> {
        let mut session = Self::new(session_id, config, scenario);
        let mut commands = replay_log.accepted_commands.clone();
        commands.sort_by_key(|command| {
            (
                command.issued_at_tick,
                command.apply_at_tick,
                command.player_id,
                command.command.clone(),
            )
        });

        for command in commands {
            while session.state.tick_id < command.issued_at_tick {
                session.advance_tick();
            }

            session.accept_command(command)?;
        }

        let target_tick = replay_log.max_apply_tick();
        while session.state.tick_id < target_tick {
            session.advance_tick();
        }

        Ok(session)
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

    pub fn pending_commands(&self) -> &[CommandEnvelope] {
        &self.pending_commands
    }

    pub fn next_random_u64(&mut self) -> u64 {
        self.state.next_random_u64()
    }

    pub fn advance_tick(&mut self) {
        self.state.tick_id = self.state.tick_id.next();
        self.event_log.push(EventRecord {
            tick_id: self.state.tick_id,
            player_id: None,
            kind: EventKind::TickAdvanced {
                tick_id: self.state.tick_id,
            },
        });

        self.apply_due_commands();
    }

    pub fn accept_command(&mut self, command: CommandEnvelope) -> Result<(), ValidationError> {
        if command.session_id != self.session_id {
            return Err(ValidationError {
                code: "session_mismatch".to_owned(),
                message: "command session does not match the target session".to_owned(),
            });
        }

        if command.apply_at_tick < self.state.tick_id {
            let error = ValidationError {
                code: "apply_in_past".to_owned(),
                message: "command apply tick is in the past".to_owned(),
            };
            self.event_log.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(command.player_id),
                kind: EventKind::CommandRejected {
                    command: command.command,
                    error: error.clone(),
                },
            });

            return Err(error);
        }

        self.replay_log.accepted_commands.push(command.clone());
        self.event_log.push(EventRecord {
            tick_id: self.state.tick_id,
            player_id: Some(command.player_id),
            kind: EventKind::CommandAccepted {
                command: command.command.clone(),
                apply_at_tick: command.apply_at_tick,
            },
        });

        if command.apply_at_tick == self.state.tick_id {
            self.apply_command(command);
        } else {
            self.pending_commands.push(command);
            self.pending_commands.sort();
        }

        Ok(())
    }

    pub fn state_hash(&self) -> u64 {
        let bytes = serde_json::to_vec(&self.state)
            .expect("authoritative state should serialize for hashing");

        stable_hash(&bytes)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot::new(
            self.session_id,
            self.config.clone(),
            self.scenario.clone(),
            self.state.clone(),
            self.event_log.clone(),
            self.replay_log.clone(),
            self.pending_commands.clone(),
        )
    }

    pub fn snapshot_json(&self) -> Result<String, serde_json::Error> {
        self.snapshot().to_json()
    }

    fn apply_due_commands(&mut self) {
        let pending_commands = std::mem::take(&mut self.pending_commands);
        let mut remaining_commands = Vec::with_capacity(pending_commands.len());

        for command in pending_commands {
            if command.apply_at_tick <= self.state.tick_id {
                self.apply_command(command);
            } else {
                remaining_commands.push(command);
            }
        }

        self.pending_commands = remaining_commands;
    }

    fn apply_command(&mut self, command: CommandEnvelope) {
        let player_id = command.player_id;
        let command_kind = command.command;

        let applied = match command_kind.clone() {
            CommandKind::NoOp | CommandKind::AdvanceTick => Ok(Vec::new()),
            CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep,
                reserved_for_training,
                reserved_for_agents,
                available,
            } => self.apply_set_throughput_budget(
                player_id,
                reserved_for_model_upkeep,
                reserved_for_training,
                reserved_for_agents,
                available,
            ),
            CommandKind::AssignAgent {
                role,
                scope,
                reserved_throughput,
            } => self.apply_assign_agent(player_id, role, scope, reserved_throughput),
            CommandKind::RegisterLocation { location_id, name } => {
                self.apply_register_location(location_id, name)
            }
            CommandKind::SetRelayStatus {
                location_id,
                relay_status,
            } => self.apply_set_relay_status(location_id, relay_status),
        };

        match applied {
            Ok(mut domain_events) => {
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: Some(player_id),
                    kind: EventKind::CommandApplied {
                        command: command_kind,
                    },
                });

                for kind in domain_events.drain(..) {
                    self.event_log.push(EventRecord {
                        tick_id: self.state.tick_id,
                        player_id: Some(player_id),
                        kind,
                    });
                }
            }
            Err(error) => {
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: Some(player_id),
                    kind: EventKind::CommandRejected {
                        command: command_kind,
                        error,
                    },
                });
            }
        }
    }

    fn apply_set_throughput_budget(
        &mut self,
        player_id: PlayerId,
        reserved_for_model_upkeep: u32,
        reserved_for_training: u32,
        reserved_for_agents: u32,
        available: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let player = self.player_state_mut(player_id)?;
        let total_reserved =
            reserved_for_model_upkeep + reserved_for_training + reserved_for_agents;

        if total_reserved > available {
            return Err(ValidationError {
                code: "throughput_overallocated".to_owned(),
                message: "reserved throughput cannot exceed available throughput".to_owned(),
            });
        }

        player.throughput.reserved_for_model_upkeep = reserved_for_model_upkeep;
        player.throughput.reserved_for_training = reserved_for_training;
        player.throughput.reserved_for_agents = reserved_for_agents;
        player.throughput.available = available;

        Ok(vec![EventKind::ThroughputBudgetSet {
            reserved_for_model_upkeep,
            reserved_for_training,
            reserved_for_agents,
            available,
        }])
    }

    fn apply_assign_agent(
        &mut self,
        player_id: PlayerId,
        role: String,
        scope: String,
        reserved_throughput: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let player = self.player_state_mut(player_id)?;
        let remaining_capacity = player
            .throughput
            .available
            .saturating_sub(player.throughput.reserved_for_model_upkeep)
            .saturating_sub(player.throughput.reserved_for_training)
            .saturating_sub(player.throughput.reserved_for_agents);

        if reserved_throughput > remaining_capacity {
            return Err(ValidationError {
                code: "insufficient_throughput".to_owned(),
                message: "agent assignment exceeds remaining throughput capacity".to_owned(),
            });
        }

        player.agents.push(crate::AgentAssignment {
            role: role.clone(),
            scope: scope.clone(),
            reserved_throughput,
        });
        player.throughput.reserved_for_agents += reserved_throughput;

        Ok(vec![EventKind::AgentAssigned {
            role,
            scope,
            reserved_throughput,
        }])
    }

    fn apply_register_location(
        &mut self,
        location_id: u32,
        name: String,
    ) -> Result<Vec<EventKind>, ValidationError> {
        if self
            .state
            .locations
            .iter()
            .any(|location| location.location_id == location_id)
        {
            return Err(ValidationError {
                code: "duplicate_location".to_owned(),
                message: "location id already exists in the session".to_owned(),
            });
        }

        self.state.locations.push(LocationState {
            location_id,
            name: name.clone(),
            kind: LocationKind::BarrenWorld,
            resource_richness: ResourceRichness::Sparse,
            energy_potential: EnergyPotential::Low,
            build_capacity: BuildCapacity::Constrained,
            strategic_position: StrategicPosition::Peripheral,
            territory: TerritoryState::Neutral,
            controller: None,
            homeworld_of: None,
            relay_status: RelayStatus::default(),
            orbital_slots: 1,
            has_environmental_hazard: false,
            hostile_remnant_present: false,
        });
        self.state
            .locations
            .sort_by_key(|location| location.location_id);

        Ok(vec![EventKind::LocationRegistered { location_id, name }])
    }

    fn apply_set_relay_status(
        &mut self,
        location_id: u32,
        relay_status: RelayStatus,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let location = self.location_state_mut(location_id)?;
        location.relay_status = relay_status.clone();

        Ok(vec![EventKind::RelayStatusChanged {
            location_id,
            relay_status,
        }])
    }

    fn player_state_mut(
        &mut self,
        player_id: PlayerId,
    ) -> Result<&mut crate::PlayerState, ValidationError> {
        self.state
            .players
            .iter_mut()
            .find(|player| player.player_id == player_id)
            .ok_or(ValidationError {
                code: "unknown_player".to_owned(),
                message: "command references a player that does not exist in the session"
                    .to_owned(),
            })
    }

    fn location_state_mut(
        &mut self,
        location_id: u32,
    ) -> Result<&mut LocationState, ValidationError> {
        self.state
            .locations
            .iter_mut()
            .find(|location| location.location_id == location_id)
            .ok_or(ValidationError {
                code: "unknown_location".to_owned(),
                message: "command references a location that does not exist in the session"
                    .to_owned(),
            })
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
