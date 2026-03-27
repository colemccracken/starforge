use crate::{
    BuildCapacity, CommandEnvelope, CommandKind, EnergyPotential, EventKind, EventRecord,
    GameConfig, GameState, InfrastructureCondition, InfrastructureKind, InfrastructureProjectKind,
    InfrastructureProjectState, LocationKind, LocationState, LocationView, LocationVisibility,
    PlayerId, PlayerStateView, RelayStatus, ReplayLog, ResourceRichness, ResourceStockpiles,
    ScenarioConfig, SessionId, Snapshot, StrategicPosition, TerritoryState, TickId, TransitKind,
    TransitState, TransitView, ValidationError,
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

        let mut session = Self {
            session_id,
            config,
            scenario,
            state: GameState::new(player_ids, seed, starting_locations, connections),
            event_log,
            replay_log: ReplayLog::default(),
            pending_commands: Vec::new(),
        };
        session.refresh_visibility();
        session
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

        let completions = self.state.advance_infrastructure_projects();
        if !completions.is_empty() {
            for completion in completions {
                let kind = match &completion.project_kind {
                    InfrastructureProjectKind::Repair { .. } => {
                        EventKind::InfrastructureRepairCompleted {
                            location_id: completion.location_id,
                            kind: completion.kind.clone(),
                        }
                    }
                    InfrastructureProjectKind::Construction { .. } => {
                        EventKind::InfrastructureConstructionCompleted {
                            location_id: completion.location_id,
                            kind: completion.kind.clone(),
                        }
                    }
                };
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: None,
                    kind,
                });
            }

            for kind in self.economy_updated_events() {
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: None,
                    kind,
                });
            }
        }

        self.state.advance_resource_extraction();

        let condition_changes = self.state.advance_infrastructure_wear();
        if !condition_changes.is_empty() {
            for change in condition_changes {
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: None,
                    kind: EventKind::InfrastructureConditionChanged {
                        location_id: change.location_id,
                        kind: change.kind,
                        condition: change.condition,
                    },
                });
            }

            for kind in self.economy_updated_events() {
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: None,
                    kind,
                });
            }
        }

        for event in self.advance_training_runs() {
            self.event_log.push(event);
        }

        for event in self.advance_pacification() {
            self.event_log.push(event);
        }

        for event in self.advance_takeover_resolution() {
            self.event_log.push(event);
        }

        self.refresh_visibility();

        let arrived_transits = self.state.resolve_arrived_transits();
        for transit in arrived_transits {
            self.handle_arrived_transit(transit);
        }
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

        if self.state.victory != crate::VictoryState::Ongoing {
            let error = ValidationError {
                code: "game_already_over".to_owned(),
                message: "the session already has a winner and cannot accept more commands"
                    .to_owned(),
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

    pub fn current_tick(&self) -> TickId {
        self.state.tick_id
    }

    pub fn issue_command_now(
        &mut self,
        player_id: PlayerId,
        command: CommandKind,
    ) -> Result<(), ValidationError> {
        let before_events = self.event_log.len();
        let command_for_lookup = command.clone();
        self.accept_command(CommandEnvelope {
            session_id: self.session_id,
            player_id,
            issued_at_tick: self.state.tick_id,
            apply_at_tick: self.state.tick_id,
            command,
        })?;

        if let Some(error) =
            self.event_log[before_events..]
                .iter()
                .find_map(|event| match &event.kind {
                    EventKind::CommandRejected { command, error }
                        if event.player_id == Some(player_id) && *command == command_for_lookup =>
                    {
                        Some(error.clone())
                    }
                    _ => None,
                })
        {
            return Err(error);
        }

        Ok(())
    }

    pub fn advance_ticks(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.advance_tick();
            if self.state.victory != crate::VictoryState::Ongoing {
                break;
            }
        }
    }

    pub fn player_view(&self, player_id: PlayerId) -> Result<PlayerStateView, ValidationError> {
        let player = self.player_state(player_id)?;
        let locations = self
            .state
            .locations
            .iter()
            .map(|location| project_location_for_player(location, player_id, &player.visibility))
            .collect();
        let transits = self
            .state
            .transits
            .iter()
            .filter(|transit| transit.player_id == player_id)
            .map(project_transit_for_player)
            .collect();

        Ok(PlayerStateView {
            tick_id: self.state.tick_id,
            player_id,
            model_tier: player.model_tier,
            economy: player.economy.clone(),
            throughput: player.throughput.clone(),
            training: player.training.clone(),
            visibility: player.visibility.clone(),
            locations,
            transits,
        })
    }

    pub fn player_events(
        &self,
        player_id: PlayerId,
        from_tick: TickId,
    ) -> Result<Vec<EventRecord>, ValidationError> {
        self.player_state(player_id)?;

        Ok(self
            .event_log
            .iter()
            .filter(|event| event.tick_id >= from_tick)
            .filter(|event| self.event_visible_to_player(player_id, event))
            .cloned()
            .collect())
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
            } => self.apply_set_throughput_budget(
                player_id,
                reserved_for_model_upkeep,
                reserved_for_training,
                reserved_for_agents,
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
            } => self.apply_set_relay_status(player_id, location_id, relay_status),
            CommandKind::QueueInfrastructureRepair {
                location_id,
                infrastructure_kind,
            } => {
                self.apply_queue_infrastructure_repair(player_id, location_id, infrastructure_kind)
            }
            CommandKind::QueueInfrastructureConstruction {
                location_id,
                infrastructure_kind,
            } => self.apply_queue_infrastructure_construction(
                player_id,
                location_id,
                infrastructure_kind,
            ),
            CommandKind::DispatchSurveyTransit {
                origin_location_id,
                destination_location_id,
            } => self.apply_dispatch_transit(
                player_id,
                origin_location_id,
                destination_location_id,
                TransitKind::Survey,
            ),
            CommandKind::DispatchPacificationTransit {
                origin_location_id,
                destination_location_id,
            } => self.apply_dispatch_transit(
                player_id,
                origin_location_id,
                destination_location_id,
                TransitKind::Pacification,
            ),
            CommandKind::DispatchClaimTransit {
                origin_location_id,
                destination_location_id,
            } => self.apply_dispatch_transit(
                player_id,
                origin_location_id,
                destination_location_id,
                TransitKind::Claim,
            ),
            CommandKind::DispatchAssaultTransit {
                origin_location_id,
                destination_location_id,
            } => self.apply_dispatch_transit(
                player_id,
                origin_location_id,
                destination_location_id,
                TransitKind::Assault,
            ),
            CommandKind::SurveyLocation { location_id } => {
                self.apply_survey_location(player_id, location_id)
            }
            CommandKind::StartTrainingRun { target_tier } => {
                self.apply_start_training_run(player_id, target_tier)
            }
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
    ) -> Result<Vec<EventKind>, ValidationError> {
        let player = self.player_state_mut(player_id)?;
        let total_reserved =
            reserved_for_model_upkeep + reserved_for_training + reserved_for_agents;

        if total_reserved > player.economy.usable_throughput {
            return Err(ValidationError {
                code: "throughput_overallocated".to_owned(),
                message: "reserved throughput cannot exceed computed usable throughput".to_owned(),
            });
        }

        player.throughput.reserved_for_model_upkeep = reserved_for_model_upkeep;
        player.throughput.reserved_for_training = reserved_for_training;
        player.throughput.reserved_for_agents = reserved_for_agents;

        Ok(vec![EventKind::ThroughputBudgetSet {
            reserved_for_model_upkeep,
            reserved_for_training,
            reserved_for_agents,
            available: player.throughput.available,
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
            infrastructure: Vec::new(),
            infrastructure_projects: Vec::new(),
            economy: Default::default(),
            stockpiles: Default::default(),
            hostile_remnant: None,
            contesting_players: Vec::new(),
            takeover_attacker: None,
            takeover_ticks_remaining: 0,
            pacification_ticks_remaining: 0,
        });
        self.state
            .locations
            .sort_by_key(|location| location.location_id);
        self.state.recompute_economy();

        Ok(vec![EventKind::LocationRegistered { location_id, name }])
    }

    fn apply_set_relay_status(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
        relay_status: RelayStatus,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let location = self.controlled_location_state_mut(player_id, location_id)?;
        location.relay_status = relay_status.clone();
        self.state.recompute_economy();

        let mut events = vec![EventKind::RelayStatusChanged {
            location_id,
            relay_status,
        }];
        events.extend(self.economy_updated_events());

        Ok(events)
    }

    fn apply_queue_infrastructure_repair(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
        infrastructure_kind: InfrastructureKind,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let (
            connected_to_empire,
            build_capacity,
            has_environmental_hazard,
            condition,
            target_index,
        ) = {
            let location = self.controlled_location_state(player_id, location_id)?;
            let queued_target_indices: Vec<usize> = location
                .infrastructure_projects
                .iter()
                .filter_map(|project| match project.kind {
                    InfrastructureProjectKind::Repair {
                        infrastructure_kind: ref queued_kind,
                        target_index,
                    } if *queued_kind == infrastructure_kind => Some(target_index),
                    _ => None,
                })
                .collect();

            let (target_index, infrastructure) = location
                .infrastructure
                .iter()
                .enumerate()
                .find(|(index, infrastructure)| {
                    infrastructure.kind == infrastructure_kind
                        && infrastructure.condition != InfrastructureCondition::Operational
                        && !queued_target_indices.contains(index)
                })
                .ok_or(ValidationError {
                    code: if location
                        .infrastructure
                        .iter()
                        .any(|infrastructure| infrastructure.kind == infrastructure_kind)
                    {
                        "infrastructure_not_damaged".to_owned()
                    } else {
                        "missing_infrastructure".to_owned()
                    },
                    message: if location
                        .infrastructure
                        .iter()
                        .any(|infrastructure| infrastructure.kind == infrastructure_kind)
                    {
                        "repair can only be queued for degraded or offline infrastructure that is not already under repair"
                            .to_owned()
                    } else {
                        "repair command references infrastructure that is not present at the location"
                            .to_owned()
                    },
                })?;

            (
                location.economy.connected_to_empire,
                location.build_capacity.clone(),
                location.has_environmental_hazard,
                infrastructure.condition.clone(),
                target_index,
            )
        };

        let cost = repair_cost(&infrastructure_kind, &condition);
        let duration_ticks = repair_duration(build_capacity, has_environmental_hazard, &condition);

        if connected_to_empire {
            let available = self
                .state
                .players
                .iter()
                .find(|player| player.player_id == player_id)
                .ok_or(ValidationError {
                    code: "unknown_player".to_owned(),
                    message: "command references a player that does not exist in the session"
                        .to_owned(),
                })?
                .economy
                .connected_stockpiles
                .clone();
            if !available.can_cover(&cost) {
                return Err(ValidationError {
                    code: "insufficient_materials".to_owned(),
                    message: "connected stockpiles cannot cover the requested repair".to_owned(),
                });
            }

            self.spend_connected_stockpiles(player_id, location_id, &cost)?;
        } else {
            let location = self.controlled_location_state_mut(player_id, location_id)?;
            if !location.stockpiles.can_cover(&cost) {
                return Err(ValidationError {
                    code: "insufficient_materials".to_owned(),
                    message: "local stockpiles cannot cover the requested repair".to_owned(),
                });
            }

            let mut remaining_cost = cost.clone();
            location.stockpiles.spend_partial(&mut remaining_cost);
        }

        let location = self.controlled_location_state_mut(player_id, location_id)?;
        location
            .infrastructure_projects
            .push(InfrastructureProjectState {
                kind: InfrastructureProjectKind::Repair {
                    infrastructure_kind: infrastructure_kind.clone(),
                    target_index,
                },
                remaining_ticks: duration_ticks,
                total_ticks: duration_ticks,
            });
        self.state.recompute_economy();

        Ok(vec![EventKind::InfrastructureRepairQueued {
            location_id,
            kind: infrastructure_kind,
            duration_ticks,
            cost,
        }])
    }

    fn apply_queue_infrastructure_construction(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
        infrastructure_kind: InfrastructureKind,
    ) -> Result<Vec<EventKind>, ValidationError> {
        if !is_buildable_infrastructure(&infrastructure_kind) {
            return Err(ValidationError {
                code: "unsupported_construction_kind".to_owned(),
                message: "construction is currently limited to core economic infrastructure"
                    .to_owned(),
            });
        }

        let (connected_to_empire, build_capacity, has_environmental_hazard) = {
            let location = self.controlled_location_state(player_id, location_id)?;
            if is_unique_infrastructure(&infrastructure_kind)
                && (location
                    .infrastructure
                    .iter()
                    .any(|infrastructure| infrastructure.kind == infrastructure_kind)
                    || location.infrastructure_projects.iter().any(|project| {
                        matches!(
                            project.kind,
                            InfrastructureProjectKind::Construction {
                                infrastructure_kind: ref queued_kind,
                            } if *queued_kind == infrastructure_kind
                        )
                    }))
            {
                return Err(ValidationError {
                    code: "duplicate_unique_infrastructure".to_owned(),
                    message:
                        "unique infrastructure cannot be constructed more than once per location"
                            .to_owned(),
                });
            }

            (
                location.economy.connected_to_empire,
                location.build_capacity.clone(),
                location.has_environmental_hazard,
            )
        };

        let cost = construction_cost(&infrastructure_kind);
        let duration_ticks = construction_duration(
            build_capacity,
            has_environmental_hazard,
            &infrastructure_kind,
        );

        if connected_to_empire {
            let available = self
                .state
                .players
                .iter()
                .find(|player| player.player_id == player_id)
                .ok_or(ValidationError {
                    code: "unknown_player".to_owned(),
                    message: "command references a player that does not exist in the session"
                        .to_owned(),
                })?
                .economy
                .connected_stockpiles
                .clone();
            if !available.can_cover(&cost) {
                return Err(ValidationError {
                    code: "insufficient_materials".to_owned(),
                    message: "connected stockpiles cannot cover the requested construction"
                        .to_owned(),
                });
            }

            self.spend_connected_stockpiles(player_id, location_id, &cost)?;
        } else {
            let location = self.controlled_location_state_mut(player_id, location_id)?;
            if !location.stockpiles.can_cover(&cost) {
                return Err(ValidationError {
                    code: "insufficient_materials".to_owned(),
                    message: "local stockpiles cannot cover the requested construction".to_owned(),
                });
            }

            let mut remaining_cost = cost.clone();
            location.stockpiles.spend_partial(&mut remaining_cost);
        }

        let location = self.controlled_location_state_mut(player_id, location_id)?;
        location
            .infrastructure_projects
            .push(InfrastructureProjectState {
                kind: InfrastructureProjectKind::Construction {
                    infrastructure_kind: infrastructure_kind.clone(),
                },
                remaining_ticks: duration_ticks,
                total_ticks: duration_ticks,
            });
        self.state.recompute_economy();

        Ok(vec![EventKind::InfrastructureConstructionQueued {
            location_id,
            kind: infrastructure_kind,
            duration_ticks,
            cost,
        }])
    }

    fn apply_dispatch_transit(
        &mut self,
        player_id: PlayerId,
        origin_location_id: u32,
        destination_location_id: u32,
        kind: TransitKind,
    ) -> Result<Vec<EventKind>, ValidationError> {
        if origin_location_id == destination_location_id {
            return Err(ValidationError {
                code: "same_origin_destination".to_owned(),
                message: "transit requires distinct origin and destination locations".to_owned(),
            });
        }

        self.controlled_location_state(player_id, origin_location_id)?;
        self.location_exists(destination_location_id)?;
        self.validate_transit_destination(player_id, destination_location_id, &kind)?;
        let travel_time_ticks =
            self.travel_time_between(origin_location_id, destination_location_id)?;
        let transit_id = self.state.next_transit_id();
        let eta_tick = TickId::new(
            self.state
                .tick_id
                .0
                .saturating_add(u64::from(travel_time_ticks)),
        );

        self.state.transits.push(TransitState {
            transit_id,
            player_id,
            origin_id: origin_location_id,
            destination_id: destination_location_id,
            eta_tick,
            kind: kind.clone(),
        });
        self.state
            .transits
            .sort_by_key(|transit| (transit.eta_tick, transit.transit_id));

        Ok(vec![EventKind::TransitDispatched {
            transit_id,
            origin_id: origin_location_id,
            destination_id: destination_location_id,
            eta_tick,
            kind,
        }])
    }

    fn apply_survey_location(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        self.location_exists(location_id)?;

        let can_survey = self.state.locations.iter().any(|location| {
            location.location_id == location_id
                && location.controller == Some(player_id)
                && location.territory == TerritoryState::Owned
        }) || self
            .player_state(player_id)?
            .visibility
            .observed_location_ids
            .contains(&location_id);
        if !can_survey {
            return Err(ValidationError {
                code: "location_not_observed".to_owned(),
                message: "survey requires current visibility at the target location".to_owned(),
            });
        }

        let player = self.player_state_mut(player_id)?;
        player.visibility.mark_surveyed(location_id);

        Ok(vec![EventKind::LocationSurveyed { location_id }])
    }

    fn apply_start_training_run(
        &mut self,
        player_id: PlayerId,
        target_tier: u8,
    ) -> Result<Vec<EventKind>, ValidationError> {
        if !(2..=5).contains(&target_tier) {
            return Err(ValidationError {
                code: "invalid_target_tier".to_owned(),
                message: "training targets must be between tiers 2 and 5".to_owned(),
            });
        }

        let player = self.player_state(player_id)?;
        if player.training.is_some() {
            return Err(ValidationError {
                code: "training_already_active".to_owned(),
                message: "a training run is already active for this player".to_owned(),
            });
        }

        if target_tier != player.model_tier.saturating_add(1) {
            return Err(ValidationError {
                code: "invalid_tier_progression".to_owned(),
                message: "training runs must target the next unlocked tier".to_owned(),
            });
        }

        let required_training_throughput = training_throughput_requirement(target_tier);
        if player.throughput.reserved_for_training < required_training_throughput {
            return Err(ValidationError {
                code: "insufficient_training_budget".to_owned(),
                message:
                    "reserved training throughput is below the requirement for the requested tier"
                        .to_owned(),
            });
        }

        let owned_worlds = self
            .state
            .locations
            .iter()
            .filter(|location| {
                location.controller == Some(player_id)
                    && location.territory == TerritoryState::Owned
            })
            .count();
        let minimum_worlds = minimum_worlds_for_tier(target_tier);
        if owned_worlds < minimum_worlds {
            return Err(ValidationError {
                code: "insufficient_owned_worlds".to_owned(),
                message: format!(
                    "training tier {target_tier} requires control of at least {minimum_worlds} worlds"
                ),
            });
        }

        if !self.state.locations.iter().any(|location| {
            location.controller == Some(player_id)
                && location.territory == TerritoryState::Owned
                && location.infrastructure.iter().any(|infrastructure| {
                    infrastructure.kind == InfrastructureKind::CommandNexus
                        && infrastructure.condition != InfrastructureCondition::Offline
                })
                && location.infrastructure.iter().any(|infrastructure| {
                    infrastructure.kind == InfrastructureKind::Datacenter
                        && infrastructure.condition != InfrastructureCondition::Offline
                })
        }) {
            return Err(ValidationError {
                code: "missing_training_site".to_owned(),
                message: "training requires at least one owned world with an operational command nexus and datacenter"
                    .to_owned(),
            });
        }

        let required_ticks = training_duration_ticks(target_tier);
        let player = self.player_state_mut(player_id)?;
        player.training = Some(crate::TrainingRunState {
            target_tier,
            progress_ticks: 0,
            required_ticks,
            required_training_throughput,
        });

        Ok(vec![EventKind::TrainingRunStarted {
            target_tier,
            required_training_throughput,
            required_ticks,
        }])
    }

    fn handle_arrived_transit(&mut self, transit: TransitState) {
        self.event_log.push(EventRecord {
            tick_id: self.state.tick_id,
            player_id: Some(transit.player_id),
            kind: EventKind::TransitArrived {
                transit_id: transit.transit_id,
                destination_id: transit.destination_id,
                kind: transit.kind.clone(),
            },
        });

        match transit.kind {
            TransitKind::Survey => {
                if let Ok(player) = self.player_state_mut(transit.player_id) {
                    player.visibility.mark_surveyed(transit.destination_id);
                }
                self.event_log.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: Some(transit.player_id),
                    kind: EventKind::LocationSurveyed {
                        location_id: transit.destination_id,
                    },
                });
            }
            TransitKind::Pacification => {
                if let Ok(events) =
                    self.resolve_pacification_arrival(transit.player_id, transit.destination_id)
                {
                    for kind in events {
                        self.event_log.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(transit.player_id),
                            kind,
                        });
                    }
                }
            }
            TransitKind::Claim => {
                if let Ok(events) =
                    self.resolve_claim_arrival(transit.player_id, transit.destination_id)
                {
                    for kind in events {
                        self.event_log.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(transit.player_id),
                            kind,
                        });
                    }
                }
            }
            TransitKind::Assault => {
                if let Ok(events) =
                    self.resolve_assault_arrival(transit.player_id, transit.destination_id)
                {
                    for kind in events {
                        self.event_log.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(transit.player_id),
                            kind,
                        });
                    }
                }
            }
        }
    }

    fn validate_transit_destination(
        &self,
        player_id: PlayerId,
        destination_location_id: u32,
        kind: &TransitKind,
    ) -> Result<(), ValidationError> {
        match kind {
            TransitKind::Survey => Ok(()),
            TransitKind::Pacification => {
                let location = self.location_state(destination_location_id)?;
                if location.territory != TerritoryState::Neutral {
                    return Err(ValidationError {
                        code: "location_not_neutral".to_owned(),
                        message: "pacification is currently limited to neutral worlds".to_owned(),
                    });
                }

                if !self
                    .player_state(player_id)?
                    .visibility
                    .surveyed_location_ids
                    .contains(&destination_location_id)
                {
                    return Err(ValidationError {
                        code: "location_not_surveyed".to_owned(),
                        message: "pacification requires a previously surveyed destination"
                            .to_owned(),
                    });
                }

                if location.hostile_remnant.is_none() {
                    return Err(ValidationError {
                        code: "no_hostile_remnant".to_owned(),
                        message: "pacification requires a hostile remnant at the destination"
                            .to_owned(),
                    });
                }

                Ok(())
            }
            TransitKind::Claim => {
                let location = self.location_state(destination_location_id)?;
                if location.territory != TerritoryState::Neutral || location.controller.is_some() {
                    return Err(ValidationError {
                        code: "location_not_claimable".to_owned(),
                        message: "claim expeditions require an unclaimed neutral world".to_owned(),
                    });
                }

                if !self
                    .player_state(player_id)?
                    .visibility
                    .surveyed_location_ids
                    .contains(&destination_location_id)
                {
                    return Err(ValidationError {
                        code: "location_not_surveyed".to_owned(),
                        message: "claiming requires a previously surveyed destination".to_owned(),
                    });
                }

                if location.hostile_remnant.is_some() {
                    return Err(ValidationError {
                        code: "hostile_remnant_present".to_owned(),
                        message: "claiming requires hostile remnants to be cleared first"
                            .to_owned(),
                    });
                }

                Ok(())
            }
            TransitKind::Assault => {
                let location = self.location_state(destination_location_id)?;
                if location.territory == TerritoryState::Contested {
                    return Err(ValidationError {
                        code: "location_already_contested".to_owned(),
                        message:
                            "assault cannot be queued for a destination that is already contested"
                                .to_owned(),
                    });
                }

                if !self
                    .player_state(player_id)?
                    .visibility
                    .surveyed_location_ids
                    .contains(&destination_location_id)
                {
                    return Err(ValidationError {
                        code: "location_not_surveyed".to_owned(),
                        message: "assault requires a previously surveyed destination".to_owned(),
                    });
                }

                if location.hostile_remnant.is_some() {
                    return Err(ValidationError {
                        code: "hostile_remnant_present".to_owned(),
                        message:
                            "assault expeditions currently require hostile remnants to be cleared first"
                                .to_owned(),
                    });
                }

                if location.controller == Some(player_id)
                    && location.territory == TerritoryState::Owned
                {
                    return Err(ValidationError {
                        code: "location_already_controlled".to_owned(),
                        message: "assault requires a non-owned destination".to_owned(),
                    });
                }

                if location.controller.is_none() {
                    return Err(ValidationError {
                        code: "destination_not_enemy_controlled".to_owned(),
                        message:
                            "assault expeditions currently require an enemy-controlled destination"
                                .to_owned(),
                    });
                }

                Ok(())
            }
        }
    }

    fn resolve_pacification_arrival(
        &mut self,
        player_id: PlayerId,
        destination_location_id: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        {
            let location = self.location_state_mut(destination_location_id)?;
            if location.territory != TerritoryState::Neutral {
                return Err(ValidationError {
                    code: "location_not_neutral".to_owned(),
                    message: "pacification arrivals require a neutral destination".to_owned(),
                });
            }

            if location.hostile_remnant.is_none() {
                return Err(ValidationError {
                    code: "no_hostile_remnant".to_owned(),
                    message: "pacification arrival found no hostile remnant to clear".to_owned(),
                });
            }

            location.hostile_remnant = None;
        }
        if let Ok(player) = self.player_state_mut(player_id) {
            player.visibility.mark_surveyed(destination_location_id);
        }

        Ok(vec![
            EventKind::HostileRemnantCleared {
                location_id: destination_location_id,
            },
            EventKind::LocationSurveyed {
                location_id: destination_location_id,
            },
        ])
    }

    fn resolve_claim_arrival(
        &mut self,
        player_id: PlayerId,
        destination_location_id: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        {
            let location = self.location_state_mut(destination_location_id)?;
            if location.territory != TerritoryState::Neutral || location.controller.is_some() {
                return Err(ValidationError {
                    code: "location_not_claimable".to_owned(),
                    message: "claim arrival found a destination that is no longer claimable"
                        .to_owned(),
                });
            }

            if location.hostile_remnant.is_some() {
                return Err(ValidationError {
                    code: "hostile_remnant_present".to_owned(),
                    message: "claim arrival cannot secure a world with active hostile remnants"
                        .to_owned(),
                });
            }

            location.territory = TerritoryState::Owned;
            location.controller = Some(player_id);
            location.relay_status = RelayStatus::Connected;
            ensure_colony_infrastructure(location);
        }

        self.state.recompute_economy();
        if let Ok(player) = self.player_state_mut(player_id) {
            player.visibility.mark_surveyed(destination_location_id);
        }

        let mut events = vec![EventKind::LocationClaimed {
            location_id: destination_location_id,
            player_id,
        }];
        events.extend(self.economy_updated_events());
        Ok(events)
    }

    fn resolve_assault_arrival(
        &mut self,
        player_id: PlayerId,
        destination_location_id: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let defender_id = {
            let location = self.location_state_mut(destination_location_id)?;
            if location.controller == Some(player_id) && location.territory == TerritoryState::Owned
            {
                return Err(ValidationError {
                    code: "location_already_controlled".to_owned(),
                    message:
                        "assault arrival found a destination already controlled by the attacker"
                            .to_owned(),
                });
            }

            if location.controller.is_none() {
                return Err(ValidationError {
                    code: "destination_not_enemy_controlled".to_owned(),
                    message: "assault arrival requires an enemy-controlled destination".to_owned(),
                });
            }

            let defender_id = location.controller;
            location.territory = TerritoryState::Contested;
            crate::state::push_unique_sorted_player_id(&mut location.contesting_players, player_id);
            location.takeover_attacker = Some(player_id);
            location.takeover_ticks_remaining = takeover_duration_ticks();
            location.pacification_ticks_remaining = 0;
            defender_id
        };

        if let Ok(attacker) = self.player_state_mut(player_id) {
            attacker.visibility.mark_contested(destination_location_id);
        }
        if let Some(defender_id) = defender_id
            && let Ok(defender) = self.player_state_mut(defender_id)
        {
            defender.visibility.mark_contested(destination_location_id);
        }

        self.state.recompute_economy();

        let mut events = vec![EventKind::LocationContested {
            location_id: destination_location_id,
            attacker_id: player_id,
            defender_id,
        }];
        events.extend(self.economy_updated_events());
        Ok(events)
    }

    fn player_state(&self, player_id: PlayerId) -> Result<&crate::PlayerState, ValidationError> {
        self.state
            .players
            .iter()
            .find(|player| player.player_id == player_id)
            .ok_or(ValidationError {
                code: "unknown_player".to_owned(),
                message: "command references a player that does not exist in the session"
                    .to_owned(),
            })
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

    fn location_state(&self, location_id: u32) -> Result<&LocationState, ValidationError> {
        self.state
            .locations
            .iter()
            .find(|location| location.location_id == location_id)
            .ok_or(ValidationError {
                code: "unknown_location".to_owned(),
                message: "command references a location that does not exist in the session"
                    .to_owned(),
            })
    }

    fn refresh_visibility(&mut self) {
        for player in &mut self.state.players {
            let owned_location_ids: Vec<u32> = self
                .state
                .locations
                .iter()
                .filter(|location| {
                    location.controller == Some(player.player_id)
                        && location.territory == TerritoryState::Owned
                })
                .map(|location| location.location_id)
                .collect();
            let contested_location_ids: Vec<u32> = self
                .state
                .locations
                .iter()
                .filter(|location| {
                    location.territory == TerritoryState::Contested
                        && (location.controller == Some(player.player_id)
                            || location.contesting_players.contains(&player.player_id))
                })
                .map(|location| location.location_id)
                .collect();

            player
                .visibility
                .refresh_owned_and_contested(&owned_location_ids, &contested_location_ids);
        }
    }

    fn advance_training_runs(&mut self) -> Vec<EventRecord> {
        if self.state.victory != crate::VictoryState::Ongoing {
            return Vec::new();
        }

        let owned_counts: Vec<(PlayerId, usize)> = self
            .state
            .players
            .iter()
            .map(|player| {
                (
                    player.player_id,
                    self.state
                        .locations
                        .iter()
                        .filter(|location| {
                            location.controller == Some(player.player_id)
                                && location.territory == TerritoryState::Owned
                        })
                        .count(),
                )
            })
            .collect();

        let mut events = Vec::new();
        for player in &mut self.state.players {
            let Some(training) = player.training.as_mut() else {
                continue;
            };

            if self.state.victory != crate::VictoryState::Ongoing {
                break;
            }

            let owned_count = owned_counts
                .iter()
                .find(|(player_id, _)| *player_id == player.player_id)
                .map(|(_, count)| *count)
                .unwrap_or(0);

            if player.throughput.reserved_for_training < training.required_training_throughput
                || owned_count < minimum_worlds_for_tier(training.target_tier)
            {
                continue;
            }

            training.progress_ticks = training.progress_ticks.saturating_add(1);
            if training.progress_ticks < training.required_ticks {
                continue;
            }

            let achieved_tier = training.target_tier;
            player.model_tier = achieved_tier;
            player.training = None;
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(player.player_id),
                kind: EventKind::TrainingRunCompleted { achieved_tier },
            });

            if achieved_tier >= 5 {
                self.state.victory = crate::VictoryState::Won {
                    winner: player.player_id,
                };
                events.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: Some(player.player_id),
                    kind: EventKind::VictoryDeclared {
                        winner: player.player_id,
                        reason: "superintelligence_ascension".to_owned(),
                    },
                });
            }
        }

        events
    }

    fn advance_takeover_resolution(&mut self) -> Vec<EventRecord> {
        if self.state.victory != crate::VictoryState::Ongoing {
            return Vec::new();
        }

        let mut captures = Vec::new();
        for location in &mut self.state.locations {
            if location.territory != TerritoryState::Contested {
                location.takeover_attacker = None;
                location.takeover_ticks_remaining = 0;
                continue;
            }

            if location.takeover_ticks_remaining == 0 {
                continue;
            }

            location.takeover_ticks_remaining -= 1;
            if location.takeover_ticks_remaining > 0 {
                continue;
            }

            let Some(attacker_id) = location.takeover_attacker else {
                continue;
            };
            let Some(defender_id) = location.controller else {
                continue;
            };

            location.territory = TerritoryState::Owned;
            location.controller = Some(attacker_id);
            location.relay_status = RelayStatus::Connected;
            location.contesting_players.clear();
            location.takeover_attacker = None;
            location.infrastructure_projects.clear();
            location.pacification_ticks_remaining = pacification_duration_ticks();

            for infrastructure in &mut location.infrastructure {
                if infrastructure.condition != InfrastructureCondition::Offline {
                    infrastructure.condition = InfrastructureCondition::Degraded;
                    infrastructure.wear = crate::state::initial_wear_for_condition(
                        &InfrastructureCondition::Degraded,
                    );
                }
            }

            captures.push((location.location_id, attacker_id, defender_id));
        }

        if captures.is_empty() {
            return Vec::new();
        }

        self.state.recompute_economy();

        let mut events = Vec::new();
        for (location_id, attacker_id, defender_id) in captures {
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: None,
                kind: EventKind::LocationCaptured {
                    location_id,
                    attacker_id,
                    defender_id,
                    pacification_ticks: pacification_duration_ticks(),
                },
            });
        }

        for kind in self.economy_updated_events() {
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: None,
                kind,
            });
        }

        if let Some(winner) = self.military_conquest_winner() {
            self.state.victory = crate::VictoryState::Won { winner };
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(winner),
                kind: EventKind::VictoryDeclared {
                    winner,
                    reason: "military_conquest".to_owned(),
                },
            });
        }

        events
    }

    fn advance_pacification(&mut self) -> Vec<EventRecord> {
        let mut completed = Vec::new();
        for location in &mut self.state.locations {
            if location.pacification_ticks_remaining == 0 {
                continue;
            }

            location.pacification_ticks_remaining -= 1;
            if location.pacification_ticks_remaining == 0
                && let Some(player_id) = location.controller
            {
                completed.push((location.location_id, player_id));
            }
        }

        if completed.is_empty() {
            return Vec::new();
        }

        self.state.recompute_economy();

        let mut events = Vec::new();
        for (location_id, player_id) in completed {
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(player_id),
                kind: EventKind::PacificationCompleted {
                    location_id,
                    player_id,
                },
            });
        }

        for kind in self.economy_updated_events() {
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: None,
                kind,
            });
        }

        events
    }

    fn military_conquest_winner(&self) -> Option<PlayerId> {
        let surviving_players: Vec<PlayerId> = self
            .state
            .players
            .iter()
            .filter(|player| {
                self.state.locations.iter().any(|location| {
                    location.controller == Some(player.player_id)
                        && location.territory == TerritoryState::Owned
                })
            })
            .map(|player| player.player_id)
            .collect();

        match surviving_players.as_slice() {
            [winner] => Some(*winner),
            _ => None,
        }
    }

    fn event_visible_to_player(&self, player_id: PlayerId, event: &EventRecord) -> bool {
        match &event.kind {
            EventKind::SessionCreated { .. } | EventKind::TickAdvanced { .. } => true,
            EventKind::CommandAccepted { .. }
            | EventKind::CommandApplied { .. }
            | EventKind::CommandRejected { .. }
            | EventKind::ThroughputBudgetSet { .. }
            | EventKind::EconomyUpdated { .. }
            | EventKind::AgentAssigned { .. }
            | EventKind::InfrastructureRepairQueued { .. }
            | EventKind::InfrastructureRepairCompleted { .. }
            | EventKind::InfrastructureConstructionQueued { .. }
            | EventKind::InfrastructureConstructionCompleted { .. }
            | EventKind::TransitDispatched { .. }
            | EventKind::TransitArrived { .. }
            | EventKind::LocationSurveyed { .. }
            | EventKind::TrainingRunStarted { .. }
            | EventKind::TrainingRunCompleted { .. } => event.player_id == Some(player_id),
            EventKind::LocationRegistered { .. } => event.player_id == Some(player_id),
            EventKind::RelayStatusChanged { location_id, .. }
            | EventKind::InfrastructureConditionChanged { location_id, .. }
            | EventKind::HostileRemnantCleared { location_id }
            | EventKind::LocationClaimed { location_id, .. }
            | EventKind::LocationContested { location_id, .. }
            | EventKind::PacificationCompleted { location_id, .. } => {
                self.location_is_observed_by_player(player_id, *location_id)
            }
            EventKind::LocationCaptured {
                location_id,
                attacker_id,
                defender_id,
                ..
            } => {
                *attacker_id == player_id
                    || *defender_id == player_id
                    || self.location_is_observed_by_player(player_id, *location_id)
            }
            EventKind::VictoryDeclared { .. } => true,
        }
    }

    fn location_is_observed_by_player(&self, player_id: PlayerId, location_id: u32) -> bool {
        self.player_state(player_id)
            .map(|player| {
                player
                    .visibility
                    .observed_location_ids
                    .contains(&location_id)
                    || self.state.locations.iter().any(|location| {
                        location.location_id == location_id
                            && ((location.controller == Some(player_id)
                                && matches!(
                                    location.territory,
                                    TerritoryState::Owned | TerritoryState::Contested
                                ))
                                || location.contesting_players.contains(&player_id))
                    })
            })
            .unwrap_or(false)
    }

    fn controlled_location_state(
        &self,
        player_id: PlayerId,
        location_id: u32,
    ) -> Result<&LocationState, ValidationError> {
        let location = self
            .state
            .locations
            .iter()
            .find(|location| location.location_id == location_id)
            .ok_or(ValidationError {
                code: "unknown_location".to_owned(),
                message: "command references a location that does not exist in the session"
                    .to_owned(),
            })?;

        if location.controller != Some(player_id) || location.territory != TerritoryState::Owned {
            return Err(ValidationError {
                code: "location_not_controlled".to_owned(),
                message: "command requires an owned location controlled by the issuing player"
                    .to_owned(),
            });
        }

        Ok(location)
    }

    fn location_exists(&self, location_id: u32) -> Result<(), ValidationError> {
        if self
            .state
            .locations
            .iter()
            .any(|location| location.location_id == location_id)
        {
            Ok(())
        } else {
            Err(ValidationError {
                code: "unknown_location".to_owned(),
                message: "command references a location that does not exist in the session"
                    .to_owned(),
            })
        }
    }

    fn travel_time_between(
        &self,
        origin_location_id: u32,
        destination_location_id: u32,
    ) -> Result<u32, ValidationError> {
        self.state
            .connections
            .iter()
            .find(|connection| {
                (connection.from_location_id == origin_location_id
                    && connection.to_location_id == destination_location_id)
                    || (connection.from_location_id == destination_location_id
                        && connection.to_location_id == origin_location_id)
            })
            .map(|connection| connection.travel_time_ticks)
            .ok_or(ValidationError {
                code: "no_route".to_owned(),
                message: "no direct route exists between the requested origin and destination"
                    .to_owned(),
            })
    }

    fn controlled_location_state_mut(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
    ) -> Result<&mut LocationState, ValidationError> {
        let location = self.location_state_mut(location_id)?;
        if location.controller != Some(player_id) || location.territory != TerritoryState::Owned {
            return Err(ValidationError {
                code: "location_not_controlled".to_owned(),
                message: "command requires an owned location controlled by the issuing player"
                    .to_owned(),
            });
        }

        Ok(location)
    }

    fn spend_connected_stockpiles(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
        cost: &ResourceStockpiles,
    ) -> Result<(), ValidationError> {
        let target_index = self
            .state
            .locations
            .iter()
            .position(|location| location.location_id == location_id)
            .ok_or(ValidationError {
                code: "unknown_location".to_owned(),
                message: "command references a location that does not exist in the session"
                    .to_owned(),
            })?;

        let mut candidate_indices = Vec::new();
        candidate_indices.push(target_index);
        candidate_indices.extend(
            self.state
                .locations
                .iter()
                .enumerate()
                .filter(|(index, location)| {
                    *index != target_index
                        && location.controller == Some(player_id)
                        && location.territory == TerritoryState::Owned
                        && location.economy.connected_to_empire
                })
                .map(|(index, _)| index),
        );

        let mut remaining_cost = cost.clone();
        for index in candidate_indices {
            self.state.locations[index]
                .stockpiles
                .spend_partial(&mut remaining_cost);
            if remaining_cost.is_zero() {
                return Ok(());
            }
        }

        Err(ValidationError {
            code: "insufficient_materials".to_owned(),
            message: "connected stockpiles cannot cover the requested project".to_owned(),
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

    fn economy_updated_events(&self) -> Vec<EventKind> {
        self.state
            .players
            .iter()
            .map(|player| EventKind::EconomyUpdated {
                player_id: player.player_id,
                total_connected_energy: player.economy.total_connected_energy,
                total_connected_datacenter_capacity: player
                    .economy
                    .total_connected_datacenter_capacity,
                usable_throughput: player.economy.usable_throughput,
            })
            .collect()
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

fn project_location_for_player(
    location: &LocationState,
    player_id: PlayerId,
    visibility: &crate::VisibilityState,
) -> LocationView {
    let is_owned =
        location.controller == Some(player_id) && location.territory == TerritoryState::Owned;
    let is_observed = visibility
        .observed_location_ids
        .contains(&location.location_id);
    let is_surveyed = visibility
        .surveyed_location_ids
        .contains(&location.location_id);

    if is_owned {
        return LocationView {
            location_id: location.location_id,
            name: location.name.clone(),
            visibility: LocationVisibility::Owned,
            territory: location.territory.clone(),
            controller: location.controller,
            contesting_players: Some(location.contesting_players.clone()),
            pacification_ticks_remaining: Some(location.pacification_ticks_remaining),
            kind: Some(location.kind.clone()),
            resource_richness: Some(location.resource_richness.clone()),
            energy_potential: Some(location.energy_potential.clone()),
            build_capacity: Some(location.build_capacity.clone()),
            relay_status: Some(location.relay_status.clone()),
            orbital_slots: Some(location.orbital_slots),
            has_environmental_hazard: Some(location.has_environmental_hazard),
            infrastructure: Some(location.infrastructure.clone()),
            infrastructure_projects: Some(location.infrastructure_projects.clone()),
            economy: Some(location.economy.clone()),
            stockpiles: Some(location.stockpiles.clone()),
            hostile_remnant_present: Some(location.hostile_remnant.is_some()),
        };
    }

    if is_observed {
        let restricted_contested_view = location.territory == TerritoryState::Contested
            && location.controller != Some(player_id);
        return LocationView {
            location_id: location.location_id,
            name: location.name.clone(),
            visibility: LocationVisibility::Observed,
            territory: location.territory.clone(),
            controller: location.controller,
            contesting_players: Some(location.contesting_players.clone()),
            pacification_ticks_remaining: if restricted_contested_view {
                None
            } else {
                Some(location.pacification_ticks_remaining)
            },
            kind: Some(location.kind.clone()),
            resource_richness: Some(location.resource_richness.clone()),
            energy_potential: Some(location.energy_potential.clone()),
            build_capacity: Some(location.build_capacity.clone()),
            relay_status: if restricted_contested_view {
                None
            } else {
                Some(location.relay_status.clone())
            },
            orbital_slots: Some(location.orbital_slots),
            has_environmental_hazard: Some(location.has_environmental_hazard),
            infrastructure: Some(if restricted_contested_view {
                sanitize_contested_infrastructure(&location.infrastructure)
            } else {
                location.infrastructure.clone()
            }),
            infrastructure_projects: if restricted_contested_view {
                None
            } else {
                Some(location.infrastructure_projects.clone())
            },
            economy: if restricted_contested_view {
                None
            } else {
                Some(location.economy.clone())
            },
            stockpiles: None,
            hostile_remnant_present: Some(location.hostile_remnant.is_some()),
        };
    }

    if is_surveyed {
        return LocationView {
            location_id: location.location_id,
            name: location.name.clone(),
            visibility: LocationVisibility::Surveyed,
            territory: TerritoryState::Obscured,
            controller: None,
            contesting_players: None,
            pacification_ticks_remaining: None,
            kind: Some(location.kind.clone()),
            resource_richness: Some(location.resource_richness.clone()),
            energy_potential: Some(location.energy_potential.clone()),
            build_capacity: Some(location.build_capacity.clone()),
            relay_status: None,
            orbital_slots: Some(location.orbital_slots),
            has_environmental_hazard: Some(location.has_environmental_hazard),
            infrastructure: None,
            infrastructure_projects: None,
            economy: None,
            stockpiles: None,
            hostile_remnant_present: Some(location.hostile_remnant.is_some()),
        };
    }

    LocationView {
        location_id: location.location_id,
        name: location.name.clone(),
        visibility: LocationVisibility::Obscured,
        territory: TerritoryState::Obscured,
        controller: None,
        contesting_players: None,
        pacification_ticks_remaining: None,
        kind: None,
        resource_richness: None,
        energy_potential: None,
        build_capacity: None,
        relay_status: None,
        orbital_slots: None,
        has_environmental_hazard: None,
        infrastructure: None,
        infrastructure_projects: None,
        economy: None,
        stockpiles: None,
        hostile_remnant_present: None,
    }
}

fn project_transit_for_player(transit: &TransitState) -> TransitView {
    TransitView {
        transit_id: transit.transit_id,
        origin_id: transit.origin_id,
        destination_id: transit.destination_id,
        eta_tick: transit.eta_tick,
        kind: transit.kind.clone(),
    }
}

fn sanitize_contested_infrastructure(
    infrastructure: &[crate::InfrastructureState],
) -> Vec<crate::InfrastructureState> {
    infrastructure
        .iter()
        .cloned()
        .map(|mut infrastructure| {
            infrastructure.wear = 0;
            infrastructure
        })
        .collect()
}

fn ensure_colony_infrastructure(location: &mut LocationState) {
    for kind in [
        InfrastructureKind::CommandNexus,
        InfrastructureKind::MiningSite,
        InfrastructureKind::RelayUplink,
    ] {
        if location
            .infrastructure
            .iter()
            .all(|infrastructure| infrastructure.kind != kind)
        {
            location.infrastructure.push(crate::InfrastructureState {
                kind,
                tier: 1,
                condition: InfrastructureCondition::Operational,
                wear: 0,
            });
        }
    }

    location
        .infrastructure
        .sort_by_key(|infrastructure| infrastructure.kind.clone());
}

fn training_throughput_requirement(target_tier: u8) -> u32 {
    match target_tier {
        2 => 20,
        3 => 35,
        4 => 50,
        5 => 70,
        _ => u32::MAX,
    }
}

fn training_duration_ticks(target_tier: u8) -> u32 {
    match target_tier {
        2 => 8,
        3 => 12,
        4 => 16,
        5 => 20,
        _ => u32::MAX,
    }
}

fn minimum_worlds_for_tier(target_tier: u8) -> usize {
    match target_tier {
        2 => 1,
        3 => 2,
        4 => 3,
        5 => 4,
        _ => usize::MAX,
    }
}

const fn takeover_duration_ticks() -> u32 {
    8
}

const fn pacification_duration_ticks() -> u32 {
    12
}

fn repair_cost(
    infrastructure_kind: &InfrastructureKind,
    condition: &InfrastructureCondition,
) -> ResourceStockpiles {
    let base_cost = match infrastructure_kind {
        InfrastructureKind::CommandNexus => ResourceStockpiles {
            common_materials: 60,
            volatiles: 20,
            rare_materials: 10,
        },
        InfrastructureKind::MiningSite => ResourceStockpiles {
            common_materials: 30,
            volatiles: 10,
            rare_materials: 0,
        },
        InfrastructureKind::EnergyProducer => ResourceStockpiles {
            common_materials: 45,
            volatiles: 15,
            rare_materials: 4,
        },
        InfrastructureKind::Datacenter => ResourceStockpiles {
            common_materials: 40,
            volatiles: 10,
            rare_materials: 4,
        },
        InfrastructureKind::RelayUplink => ResourceStockpiles {
            common_materials: 25,
            volatiles: 8,
            rare_materials: 2,
        },
        InfrastructureKind::ShipyardRing => ResourceStockpiles {
            common_materials: 70,
            volatiles: 20,
            rare_materials: 10,
        },
        InfrastructureKind::MilitaryWorks => ResourceStockpiles {
            common_materials: 60,
            volatiles: 16,
            rare_materials: 8,
        },
        InfrastructureKind::GroundDefenseSite => ResourceStockpiles {
            common_materials: 35,
            volatiles: 10,
            rare_materials: 4,
        },
    };

    let multiplier = match condition {
        InfrastructureCondition::Operational => 0,
        InfrastructureCondition::Degraded => 1,
        InfrastructureCondition::Offline => 2,
    };

    ResourceStockpiles {
        common_materials: base_cost.common_materials.saturating_mul(multiplier),
        volatiles: base_cost.volatiles.saturating_mul(multiplier),
        rare_materials: base_cost.rare_materials.saturating_mul(multiplier),
    }
}

fn repair_duration(
    build_capacity: BuildCapacity,
    has_environmental_hazard: bool,
    condition: &InfrastructureCondition,
) -> u32 {
    let base_duration: i32 = match condition {
        InfrastructureCondition::Operational => 0,
        InfrastructureCondition::Degraded => 3,
        InfrastructureCondition::Offline => 5,
    };
    let build_adjustment = match build_capacity {
        BuildCapacity::Constrained => 1,
        BuildCapacity::Standard => 0,
        BuildCapacity::Expansive => -1,
    };
    let hazard_adjustment = if has_environmental_hazard { 1 } else { 0 };

    (base_duration + build_adjustment + hazard_adjustment).max(1) as u32
}

fn construction_cost(infrastructure_kind: &InfrastructureKind) -> ResourceStockpiles {
    match infrastructure_kind {
        InfrastructureKind::MiningSite => ResourceStockpiles {
            common_materials: 70,
            volatiles: 20,
            rare_materials: 0,
        },
        InfrastructureKind::EnergyProducer => ResourceStockpiles {
            common_materials: 90,
            volatiles: 30,
            rare_materials: 8,
        },
        InfrastructureKind::Datacenter => ResourceStockpiles {
            common_materials: 80,
            volatiles: 20,
            rare_materials: 8,
        },
        InfrastructureKind::RelayUplink => ResourceStockpiles {
            common_materials: 50,
            volatiles: 15,
            rare_materials: 4,
        },
        InfrastructureKind::CommandNexus
        | InfrastructureKind::ShipyardRing
        | InfrastructureKind::MilitaryWorks
        | InfrastructureKind::GroundDefenseSite => ResourceStockpiles::default(),
    }
}

fn construction_duration(
    build_capacity: BuildCapacity,
    has_environmental_hazard: bool,
    infrastructure_kind: &InfrastructureKind,
) -> u32 {
    let base_duration: i32 = match infrastructure_kind {
        InfrastructureKind::MiningSite => 3,
        InfrastructureKind::EnergyProducer => 4,
        InfrastructureKind::Datacenter => 4,
        InfrastructureKind::RelayUplink => 3,
        InfrastructureKind::CommandNexus
        | InfrastructureKind::ShipyardRing
        | InfrastructureKind::MilitaryWorks
        | InfrastructureKind::GroundDefenseSite => 0,
    };
    let build_adjustment = match build_capacity {
        BuildCapacity::Constrained => 1,
        BuildCapacity::Standard => 0,
        BuildCapacity::Expansive => -1,
    };
    let hazard_adjustment = if has_environmental_hazard { 1 } else { 0 };

    (base_duration + build_adjustment + hazard_adjustment).max(1) as u32
}

const fn is_buildable_infrastructure(infrastructure_kind: &InfrastructureKind) -> bool {
    matches!(
        infrastructure_kind,
        InfrastructureKind::MiningSite
            | InfrastructureKind::EnergyProducer
            | InfrastructureKind::Datacenter
            | InfrastructureKind::RelayUplink
    )
}

const fn is_unique_infrastructure(infrastructure_kind: &InfrastructureKind) -> bool {
    matches!(
        infrastructure_kind,
        InfrastructureKind::CommandNexus | InfrastructureKind::RelayUplink
    )
}
