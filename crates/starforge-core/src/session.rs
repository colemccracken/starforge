use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};

use crate::{
    BuildCapacity, CommandEnvelope, CommandKind, EnergyPotential, EventKind, EventRecord,
    GameConfig, GameState, IndexedEventRecord, InfrastructureCondition, InfrastructureKind,
    InfrastructureProjectKind, InfrastructureProjectState, InfrastructureProjectView,
    LocationConnection, LocationKind, LocationState, LocationView, LocationVisibility,
    MAX_INFRASTRUCTURE_LEVEL, PlayerId, PlayerStateView, RelayStatus, ReplayLog, ResearchBranch,
    ResourceRichness, ResourceStockpiles, ScenarioConfig, SessionId, Snapshot, StrategicPosition,
    TerritoryState, TickId, TransitKind, TransitState, TransitView, ValidationError,
    command::format_reserved_throughput_shortfall, construction_preview,
    grouped_infrastructure_families, has_max_infrastructure_level,
    has_queued_infrastructure_family_project, infrastructure_family_kinds,
    infrastructure_family_level, repair_preview, research_preview,
    select_infrastructure_repair_target, strategic_strike_cost, training_preview,
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

    pub fn from_snapshot_json(json: &str) -> Result<Self, crate::SnapshotError> {
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
                    InfrastructureProjectKind::Development { .. } => {
                        EventKind::InfrastructureDevelopmentCompleted {
                            location_id: completion.location_id,
                            kind: completion.kind.clone(),
                            achieved_level: infrastructure_family_level(
                                self.state
                                    .locations
                                    .iter()
                                    .find(|location| location.location_id == completion.location_id)
                                    .map(|location| location.infrastructure.as_slice())
                                    .unwrap_or(&[]),
                                completion.kind.clone(),
                            ),
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

        for event in self.advance_research_projects() {
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

        for event in self.advance_command_collapse() {
            self.event_log.push(event);
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

        if matches!(
            self.player_state(command.player_id)?.collapse,
            crate::CommandCollapseState::Defeated
        ) {
            let error = ValidationError {
                code: "player_defeated".to_owned(),
                message: "defeated players can no longer issue commands".to_owned(),
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
        let locations: Vec<LocationView> = self
            .state
            .locations
            .iter()
            .map(|location| project_location_for_player(location, player_id, &player.visibility))
            .collect();
        let routes = project_routes_for_player(&self.state.connections, &locations);
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
            research: player.research.clone(),
            training: player.training.clone(),
            collapse: player.collapse.clone(),
            visibility: player.visibility.clone(),
            locations,
            routes,
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

    pub fn player_events_from_index(
        &self,
        player_id: PlayerId,
        from_event_index: usize,
    ) -> Result<Vec<IndexedEventRecord>, ValidationError> {
        self.player_state(player_id)?;

        Ok(self
            .event_log
            .iter()
            .enumerate()
            .skip(from_event_index)
            .filter(|(_, event)| self.event_visible_to_player(player_id, event))
            .map(|(event_index, record)| IndexedEventRecord {
                event_index,
                record: record.clone(),
            })
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
                reserved_for_research,
                reserved_for_agents,
            } => self.apply_set_throughput_budget(
                player_id,
                reserved_for_model_upkeep,
                reserved_for_training,
                reserved_for_research,
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
            CommandKind::QueueInfrastructureDevelopment {
                location_id,
                infrastructure_kind,
            } => self.apply_queue_infrastructure_development(
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
            CommandKind::DispatchStrategicStrike {
                origin_location_id,
                destination_location_id,
            } => self.apply_dispatch_transit(
                player_id,
                origin_location_id,
                destination_location_id,
                TransitKind::StrategicStrike,
            ),
            CommandKind::SurveyLocation { location_id } => {
                self.apply_survey_location(player_id, location_id)
            }
            CommandKind::StartTrainingRun { target_tier } => {
                self.apply_start_training_run(player_id, target_tier)
            }
            CommandKind::StartResearchProject {
                branch,
                target_level,
            } => self.apply_start_research_project(player_id, branch, target_level),
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
        reserved_for_research: u32,
        reserved_for_agents: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let player = self.player_state_mut(player_id)?;
        let total_reserved = reserved_for_model_upkeep
            + reserved_for_training
            + reserved_for_research
            + reserved_for_agents;

        if total_reserved > player.economy.usable_throughput {
            return Err(ValidationError {
                code: "throughput_overallocated".to_owned(),
                message: "reserved throughput cannot exceed computed usable throughput".to_owned(),
            });
        }

        player.throughput.reserved_for_model_upkeep = reserved_for_model_upkeep;
        player.throughput.reserved_for_training = reserved_for_training;
        player.throughput.reserved_for_research = reserved_for_research;
        player.throughput.reserved_for_agents = reserved_for_agents;

        Ok(vec![EventKind::ThroughputBudgetSet {
            reserved_for_model_upkeep,
            reserved_for_training,
            reserved_for_research,
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
            .saturating_sub(player.throughput.reserved_for_research)
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
            if has_queued_infrastructure_family_project(
                &location.infrastructure_projects,
                infrastructure_kind.clone(),
            ) {
                return Err(ValidationError {
                    code: "infrastructure_project_already_queued".to_owned(),
                    message:
                        "a repair or development project is already queued for this infrastructure family"
                            .to_owned(),
                });
            }

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

            let (target_index, condition) = select_infrastructure_repair_target(
                &location.infrastructure,
                infrastructure_kind.clone(),
                &queued_target_indices,
            )
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
                condition,
                target_index,
            )
        };
        let industry_level = self.player_state(player_id)?.research.industry_level;

        let preview = repair_preview(
            &infrastructure_kind,
            &condition,
            build_capacity,
            has_environmental_hazard,
            industry_level,
        );
        let cost = preview.cost.clone();
        let duration_ticks = preview.duration_ticks;

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

    fn apply_queue_infrastructure_development(
        &mut self,
        player_id: PlayerId,
        location_id: u32,
        infrastructure_kind: InfrastructureKind,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let (connected_to_empire, build_capacity, has_environmental_hazard, target_level) = {
            let location = self.controlled_location_state(player_id, location_id)?;
            if has_queued_infrastructure_family_project(
                &location.infrastructure_projects,
                infrastructure_kind.clone(),
            ) {
                return Err(ValidationError {
                    code: "infrastructure_project_already_queued".to_owned(),
                    message:
                        "a repair or development project is already queued for this infrastructure family"
                            .to_owned(),
                });
            }
            let current_level =
                infrastructure_family_level(&location.infrastructure, infrastructure_kind.clone());
            if infrastructure_kind == InfrastructureKind::CommandNexus && current_level == 0 {
                return Err(ValidationError {
                    code: "manual_nexus_not_allowed".to_owned(),
                    message:
                        "command nexus progression must start from seeded colony or homeworld infrastructure"
                            .to_owned(),
                });
            }
            if has_max_infrastructure_level(&location.infrastructure, infrastructure_kind.clone()) {
                return Err(ValidationError {
                    code: "max_infrastructure_level".to_owned(),
                    message: format!(
                        "{infrastructure_kind:?} is already at the maximum level of {MAX_INFRASTRUCTURE_LEVEL}"
                    ),
                });
            }

            (
                location.economy.connected_to_empire,
                location.build_capacity.clone(),
                location.has_environmental_hazard,
                current_level.saturating_add(1),
            )
        };
        let industry_level = self.player_state(player_id)?.research.industry_level;

        let preview = construction_preview(
            &infrastructure_kind,
            build_capacity,
            has_environmental_hazard,
            industry_level,
        );
        let cost = preview.cost.clone();
        let duration_ticks = preview.duration_ticks;

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
                    message: "connected stockpiles cannot cover the requested development"
                        .to_owned(),
                });
            }

            self.spend_connected_stockpiles(player_id, location_id, &cost)?;
        } else {
            let location = self.controlled_location_state_mut(player_id, location_id)?;
            if !location.stockpiles.can_cover(&cost) {
                return Err(ValidationError {
                    code: "insufficient_materials".to_owned(),
                    message: "local stockpiles cannot cover the requested development".to_owned(),
                });
            }

            let mut remaining_cost = cost.clone();
            location.stockpiles.spend_partial(&mut remaining_cost);
        }

        let location = self.controlled_location_state_mut(player_id, location_id)?;
        location
            .infrastructure_projects
            .push(InfrastructureProjectState {
                kind: InfrastructureProjectKind::Development {
                    infrastructure_kind: infrastructure_kind.clone(),
                },
                remaining_ticks: duration_ticks,
                total_ticks: duration_ticks,
            });
        self.state.recompute_economy();

        Ok(vec![EventKind::InfrastructureDevelopmentQueued {
            location_id,
            kind: infrastructure_kind,
            target_level,
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
        self.validate_transit_origin(player_id, origin_location_id, &kind)?;
        self.validate_transit_destination(player_id, destination_location_id, &kind)?;
        if kind == TransitKind::StrategicStrike {
            self.pay_strategic_strike_cost(player_id, origin_location_id)?;
            self.state.recompute_economy();
        }
        let travel_time_ticks =
            self.travel_time_for_transit(origin_location_id, destination_location_id, &kind)?;
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

    fn validate_transit_origin(
        &self,
        player_id: PlayerId,
        origin_location_id: u32,
        kind: &TransitKind,
    ) -> Result<(), ValidationError> {
        let origin = self.controlled_location_state(player_id, origin_location_id)?;

        match kind {
            TransitKind::Assault => {
                if self.assault_strength(player_id, origin) == 0 {
                    return Err(ValidationError {
                        code: "missing_assault_staging".to_owned(),
                        message: "assaults require an operational military works or shipyard at the origin"
                            .to_owned(),
                    });
                }
            }
            TransitKind::StrategicStrike => {
                if self.strategic_strike_strength(player_id, origin) == 0 {
                    return Err(ValidationError {
                        code: "missing_strike_staging".to_owned(),
                        message: "strategic strikes require an operational military works or shipyard at the origin"
                            .to_owned(),
                    });
                }
            }
            TransitKind::Survey | TransitKind::Pacification | TransitKind::Claim => {}
        }

        Ok(())
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

        let preview = training_preview(target_tier, player.research.models_level);
        let required_training_throughput = preview.required_throughput;
        if player.throughput.reserved_for_training < required_training_throughput {
            return Err(ValidationError {
                code: "insufficient_training_budget".to_owned(),
                message: format_reserved_throughput_shortfall(
                    "training",
                    player.throughput.reserved_for_training,
                    required_training_throughput,
                ),
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
        let minimum_worlds = preview.minimum_worlds;
        if owned_worlds < minimum_worlds {
            return Err(ValidationError {
                code: "insufficient_owned_worlds".to_owned(),
                message: format!(
                    "training tier {target_tier} requires control of at least {minimum_worlds} worlds"
                ),
            });
        }

        if !self.player_has_research_site(player_id) {
            return Err(ValidationError {
                code: "missing_training_site".to_owned(),
                message: "training requires at least one owned world with an operational command nexus and datacenter"
                    .to_owned(),
            });
        }

        let ascension_site_location_id = if target_tier >= 5 {
            Some(self.select_ascension_site(player_id).ok_or(ValidationError {
                code: "missing_ascension_site".to_owned(),
                message: "tier 5 ascension requires a connected owned world with an active command nexus and datacenter"
                    .to_owned(),
            })?)
        } else {
            None
        };
        let required_ticks = preview.required_ticks;
        let player = self.player_state_mut(player_id)?;
        player.training = Some(crate::TrainingRunState {
            target_tier,
            progress_ticks: 0,
            required_ticks,
            required_training_throughput,
            ascension_site_location_id,
        });

        let mut events = vec![EventKind::TrainingRunStarted {
            target_tier,
            required_training_throughput,
            required_ticks,
        }];
        if let Some(location_id) = ascension_site_location_id {
            events.push(EventKind::AscensionStarted {
                player_id,
                location_id,
                required_training_throughput,
                required_ticks,
            });
        }

        Ok(events)
    }

    fn apply_start_research_project(
        &mut self,
        player_id: PlayerId,
        branch: ResearchBranch,
        target_level: u8,
    ) -> Result<Vec<EventKind>, ValidationError> {
        if !(1..=3).contains(&target_level) {
            return Err(ValidationError {
                code: "invalid_target_level".to_owned(),
                message: "research targets must be between levels 1 and 3".to_owned(),
            });
        }

        let player = self.player_state(player_id)?;
        if player.research.active_project.is_some() {
            return Err(ValidationError {
                code: "research_already_active".to_owned(),
                message: "a research project is already active for this player".to_owned(),
            });
        }

        let current_level = player.research.level_for(branch);
        if target_level != current_level.saturating_add(1) {
            return Err(ValidationError {
                code: "invalid_research_progression".to_owned(),
                message: "research projects must target the next unlocked level".to_owned(),
            });
        }

        let preview = research_preview(target_level);
        let required_research_throughput = preview.required_throughput;
        if player.throughput.reserved_for_research < required_research_throughput {
            return Err(ValidationError {
                code: "insufficient_research_budget".to_owned(),
                message: format_reserved_throughput_shortfall(
                    "research",
                    player.throughput.reserved_for_research,
                    required_research_throughput,
                ),
            });
        }

        if !self.player_has_research_site(player_id) {
            return Err(ValidationError {
                code: "missing_research_site".to_owned(),
                message: "research requires at least one owned world with an operational command nexus and datacenter"
                    .to_owned(),
            });
        }

        let required_ticks = preview.required_ticks;
        let player = self.player_state_mut(player_id)?;
        player.research.active_project = Some(crate::ResearchProjectState {
            branch,
            target_level,
            progress_ticks: 0,
            required_ticks,
            required_research_throughput,
        });

        Ok(vec![EventKind::ResearchProjectStarted {
            branch,
            target_level,
            required_research_throughput,
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
                if let Ok(events) = self.resolve_assault_arrival(
                    transit.player_id,
                    transit.origin_id,
                    transit.destination_id,
                ) {
                    for kind in events {
                        self.event_log.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(transit.player_id),
                            kind,
                        });
                    }
                }
            }
            TransitKind::StrategicStrike => {
                if let Ok(events) = self.resolve_strategic_strike_arrival(
                    transit.player_id,
                    transit.origin_id,
                    transit.destination_id,
                ) {
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
            TransitKind::StrategicStrike => {
                let location = self.location_state(destination_location_id)?;
                if !self
                    .player_state(player_id)?
                    .visibility
                    .surveyed_location_ids
                    .contains(&destination_location_id)
                {
                    return Err(ValidationError {
                        code: "location_not_surveyed".to_owned(),
                        message: "strategic strikes require a previously surveyed destination"
                            .to_owned(),
                    });
                }

                if location.territory == TerritoryState::Destroyed {
                    return Err(ValidationError {
                        code: "location_already_destroyed".to_owned(),
                        message: "strategic strikes cannot target a destroyed world".to_owned(),
                    });
                }

                if location.controller == Some(player_id) {
                    return Err(ValidationError {
                        code: "location_already_controlled".to_owned(),
                        message: "strategic strikes require a non-owned destination".to_owned(),
                    });
                }

                if location.controller.is_none() {
                    return Err(ValidationError {
                        code: "destination_not_enemy_controlled".to_owned(),
                        message:
                            "strategic strikes currently require an enemy-controlled destination"
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
        origin_location_id: u32,
        destination_location_id: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let attack_strength = {
            let origin = self.controlled_location_state(player_id, origin_location_id)?;
            self.assault_strength(player_id, origin)
        };
        let defender_id = {
            let location = self.location_state(destination_location_id)?;
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

            location.controller
        };

        let defender_id = defender_id.expect("enemy-controlled destination should have a defender");
        let defense_strength = {
            let destination = self.location_state(destination_location_id)?;
            self.defense_strength(defender_id, destination)
        };

        if attack_strength <= defense_strength {
            return Ok(vec![EventKind::AssaultRepelled {
                location_id: destination_location_id,
                attacker_id: player_id,
                defender_id,
            }]);
        }

        let takeover_ticks = takeover_duration_ticks(
            player_id,
            defender_id,
            attack_strength,
            defense_strength,
            &self.state,
        );

        {
            let location = self.location_state_mut(destination_location_id)?;
            location.territory = TerritoryState::Contested;
            crate::state::push_unique_sorted_player_id(&mut location.contesting_players, player_id);
            location.takeover_attacker = Some(player_id);
            location.takeover_ticks_remaining = takeover_ticks;
            location.pacification_ticks_remaining = 0;
        }

        if let Ok(attacker) = self.player_state_mut(player_id) {
            attacker.visibility.mark_contested(destination_location_id);
        }
        if let Ok(defender) = self.player_state_mut(defender_id) {
            defender.visibility.mark_contested(destination_location_id);
        }

        self.state.recompute_economy();

        let mut events = vec![EventKind::LocationContested {
            location_id: destination_location_id,
            attacker_id: player_id,
            defender_id: Some(defender_id),
        }];
        events.extend(self.economy_updated_events());
        Ok(events)
    }

    fn resolve_strategic_strike_arrival(
        &mut self,
        player_id: PlayerId,
        origin_location_id: u32,
        destination_location_id: u32,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let strike_strength = {
            let origin = self.controlled_location_state(player_id, origin_location_id)?;
            self.strategic_strike_strength(player_id, origin)
        };

        let (defender_id, intercepted) = {
            let location = self.location_state(destination_location_id)?;
            if location.territory == TerritoryState::Destroyed {
                return Err(ValidationError {
                    code: "location_already_destroyed".to_owned(),
                    message: "strategic strike arrival found a world already destroyed".to_owned(),
                });
            }

            let defender_id = location.controller.ok_or(ValidationError {
                code: "destination_not_enemy_controlled".to_owned(),
                message: "strategic strike arrival requires an enemy-controlled destination"
                    .to_owned(),
            })?;

            if defender_id == player_id {
                return Err(ValidationError {
                    code: "location_already_controlled".to_owned(),
                    message:
                        "strategic strike arrival found a destination controlled by the attacker"
                            .to_owned(),
                });
            }

            let intercepted =
                self.strategic_strike_defense_strength(defender_id, location) >= strike_strength;

            (defender_id, intercepted)
        };

        if intercepted {
            return Ok(vec![EventKind::StrategicStrikeIntercepted {
                location_id: destination_location_id,
                attacker_id: player_id,
                defender_id,
            }]);
        }

        {
            let location = self.location_state_mut(destination_location_id)?;
            location.territory = TerritoryState::Destroyed;
            location.controller = None;
            location.relay_status = RelayStatus::Disconnected;
            location.infrastructure.clear();
            location.infrastructure_projects.clear();
            location.stockpiles = ResourceStockpiles::default();
            location.hostile_remnant = None;
            location.contesting_players.clear();
            location.takeover_attacker = None;
            location.takeover_ticks_remaining = 0;
            location.pacification_ticks_remaining = 0;
        }

        self.state.recompute_economy();

        let mut events = vec![EventKind::LocationDestroyed {
            location_id: destination_location_id,
            attacker_id: player_id,
            defender_id,
        }];
        events.extend(self.economy_updated_events());

        if let Some(winner) = self.military_conquest_winner() {
            self.state.victory = crate::VictoryState::Won { winner };
            events.push(EventKind::VictoryDeclared {
                winner,
                reason: "military_conquest".to_owned(),
            });
        }

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

    fn select_ascension_site(&self, player_id: PlayerId) -> Option<u32> {
        self.state
            .locations
            .iter()
            .find(|location| self.is_valid_ascension_site(player_id, location.location_id))
            .map(|location| location.location_id)
    }

    fn is_valid_ascension_site(&self, player_id: PlayerId, location_id: u32) -> bool {
        self.state.locations.iter().any(|location| {
            location.location_id == location_id
                && location.controller == Some(player_id)
                && location.territory == TerritoryState::Owned
                && location.economy.connected_to_empire
                && location.infrastructure.iter().any(|infrastructure| {
                    infrastructure.kind == InfrastructureKind::CommandNexus
                        && infrastructure.condition != InfrastructureCondition::Offline
                })
                && location.infrastructure.iter().any(|infrastructure| {
                    infrastructure.kind == InfrastructureKind::Datacenter
                        && infrastructure.condition != InfrastructureCondition::Offline
                })
        })
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
        for index in 0..self.state.players.len() {
            if self.state.victory != crate::VictoryState::Ongoing {
                break;
            }

            let player_id = self.state.players[index].player_id;
            let Some(training_snapshot) = self.state.players[index].training.clone() else {
                continue;
            };

            let owned_count = owned_counts
                .iter()
                .find(|(candidate_id, _)| *candidate_id == player_id)
                .map(|(_, count)| *count)
                .unwrap_or(0);

            let insufficient_training_budget =
                self.state.players[index].throughput.reserved_for_training
                    < training_snapshot.required_training_throughput;
            let insufficient_available_throughput = self.state.players[index].throughput.available
                < training_snapshot.required_training_throughput;
            let insufficient_worlds = owned_count
                < training_preview(
                    training_snapshot.target_tier,
                    self.state.players[index].research.models_level,
                )
                .minimum_worlds;

            if training_snapshot.target_tier >= 5 {
                let Some(location_id) = training_snapshot.ascension_site_location_id else {
                    self.state.players[index].training = None;
                    events.push(EventRecord {
                        tick_id: self.state.tick_id,
                        player_id: Some(player_id),
                        kind: EventKind::AscensionInterrupted {
                            player_id,
                            location_id: 0,
                            reason: "missing_site".to_owned(),
                        },
                    });
                    continue;
                };

                if insufficient_training_budget
                    || insufficient_available_throughput
                    || insufficient_worlds
                    || !self.is_valid_ascension_site(player_id, location_id)
                {
                    let reason =
                        if insufficient_training_budget || insufficient_available_throughput {
                            "throughput_below_threshold"
                        } else if insufficient_worlds {
                            "insufficient_owned_worlds"
                        } else {
                            "site_unavailable"
                        };
                    self.state.players[index].training = None;
                    events.push(EventRecord {
                        tick_id: self.state.tick_id,
                        player_id: Some(player_id),
                        kind: EventKind::AscensionInterrupted {
                            player_id,
                            location_id,
                            reason: reason.to_owned(),
                        },
                    });
                    continue;
                }
            } else if insufficient_training_budget
                || insufficient_available_throughput
                || insufficient_worlds
            {
                continue;
            }

            let player = &mut self.state.players[index];
            let Some(training) = player.training.as_mut() else {
                continue;
            };
            training.progress_ticks = training.progress_ticks.saturating_add(1);
            if training.progress_ticks < training.required_ticks {
                continue;
            }

            let achieved_tier = training.target_tier;
            player.model_tier = achieved_tier;
            player.training = None;
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(player_id),
                kind: EventKind::TrainingRunCompleted { achieved_tier },
            });

            if achieved_tier >= 5 {
                self.state.victory = crate::VictoryState::Won { winner: player_id };
                events.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: Some(player_id),
                    kind: EventKind::VictoryDeclared {
                        winner: player_id,
                        reason: "superintelligence_ascension".to_owned(),
                    },
                });
            }
        }

        events
    }

    fn advance_research_projects(&mut self) -> Vec<EventRecord> {
        let mut events = Vec::new();

        for index in 0..self.state.players.len() {
            let player_id = self.state.players[index].player_id;
            let Some(project_snapshot) = self.state.players[index].research.active_project.clone()
            else {
                continue;
            };

            let insufficient_research_budget =
                self.state.players[index].throughput.reserved_for_research
                    < project_snapshot.required_research_throughput;
            let insufficient_available_throughput = self.state.players[index].throughput.available
                < project_snapshot.required_research_throughput;

            if insufficient_research_budget
                || insufficient_available_throughput
                || !self.player_has_research_site(player_id)
            {
                continue;
            }

            let player = &mut self.state.players[index];
            let Some(project) = player.research.active_project.as_mut() else {
                continue;
            };
            project.progress_ticks = project.progress_ticks.saturating_add(1);
            if project.progress_ticks < project.required_ticks {
                continue;
            }

            let branch = project.branch;
            let achieved_level = project.target_level;
            player.research.set_level(branch, achieved_level);
            player.research.active_project = None;

            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(player_id),
                kind: EventKind::ResearchProjectCompleted {
                    branch,
                    achieved_level,
                },
            });
        }

        events
    }

    fn advance_command_collapse(&mut self) -> Vec<EventRecord> {
        if self.state.victory != crate::VictoryState::Ongoing {
            return Vec::new();
        }

        let active_nexus_by_player: Vec<(PlayerId, bool)> = self
            .state
            .players
            .iter()
            .map(|player| {
                (
                    player.player_id,
                    self.player_has_active_command_nexus(player.player_id),
                )
            })
            .collect();

        let mut events = Vec::new();
        let mut defeated_players = Vec::new();
        let mut collapse_winner = None;

        for index in 0..self.state.players.len() {
            let player_id = self.state.players[index].player_id;
            if !self.player_has_owned_presence(player_id) {
                continue;
            }
            let has_active_nexus = active_nexus_by_player
                .iter()
                .find(|(candidate_id, _)| *candidate_id == player_id)
                .map(|(_, has_active_nexus)| *has_active_nexus)
                .unwrap_or(false);

            match self.state.players[index].collapse.clone() {
                crate::CommandCollapseState::Stable => {
                    if !has_active_nexus {
                        let ticks_remaining = collapse_countdown_ticks(
                            self.state.players[index].research.resilience_level,
                        );
                        self.state.players[index].collapse =
                            crate::CommandCollapseState::Collapsing { ticks_remaining };
                        events.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(player_id),
                            kind: EventKind::CommandCollapseStarted {
                                player_id,
                                ticks_remaining,
                            },
                        });
                    }
                }
                crate::CommandCollapseState::Collapsing { ticks_remaining } => {
                    if has_active_nexus {
                        self.state.players[index].collapse = crate::CommandCollapseState::Stable;
                        events.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(player_id),
                            kind: EventKind::CommandCollapseRecovered { player_id },
                        });
                    } else if ticks_remaining <= 1 {
                        self.state.players[index].collapse = crate::CommandCollapseState::Defeated;
                        self.state.players[index].training = None;
                        self.state.players[index].research.active_project = None;
                        self.state.players[index].agents.clear();
                        self.state.players[index].throughput.reserved_for_agents = 0;
                        self.state.players[index].throughput.reserved_for_training = 0;
                        self.state.players[index].throughput.reserved_for_research = 0;
                        defeated_players.push(player_id);
                        events.push(EventRecord {
                            tick_id: self.state.tick_id,
                            player_id: Some(player_id),
                            kind: EventKind::PlayerDefeated {
                                player_id,
                                reason: "command_collapse".to_owned(),
                            },
                        });
                        if let Some(winner) = self.remaining_non_defeated_player() {
                            collapse_winner = Some(winner);
                            break;
                        }
                    } else {
                        self.state.players[index].collapse =
                            crate::CommandCollapseState::Collapsing {
                                ticks_remaining: ticks_remaining - 1,
                            };
                    }
                }
                crate::CommandCollapseState::Defeated => {}
            }
        }

        if !defeated_players.is_empty() {
            for player_id in defeated_players {
                self.neutralize_defeated_player_assets(player_id);
            }

            self.state.recompute_economy();
            for kind in self.economy_updated_events() {
                events.push(EventRecord {
                    tick_id: self.state.tick_id,
                    player_id: None,
                    kind,
                });
            }
        }

        if let Some(winner) = collapse_winner.or_else(|| {
            if self
                .state
                .players
                .iter()
                .any(|player| matches!(player.collapse, crate::CommandCollapseState::Defeated))
            {
                self.remaining_non_defeated_player()
            } else {
                None
            }
        }) {
            self.state.victory = crate::VictoryState::Won { winner };
            events.push(EventRecord {
                tick_id: self.state.tick_id,
                player_id: Some(winner),
                kind: EventKind::VictoryDeclared {
                    winner,
                    reason: "command_collapse".to_owned(),
                },
            });
        }

        events
    }

    fn player_has_active_command_nexus(&self, player_id: PlayerId) -> bool {
        self.state.locations.iter().any(|location| {
            location.controller == Some(player_id)
                && location.territory == TerritoryState::Owned
                && location.infrastructure.iter().any(|infrastructure| {
                    infrastructure.kind == InfrastructureKind::CommandNexus
                        && infrastructure.condition != InfrastructureCondition::Offline
                })
        })
    }

    fn player_has_owned_presence(&self, player_id: PlayerId) -> bool {
        self.state.locations.iter().any(|location| {
            location.controller == Some(player_id)
                && location.territory != TerritoryState::Destroyed
        })
    }

    fn neutralize_defeated_player_assets(&mut self, player_id: PlayerId) {
        self.pending_commands
            .retain(|command| command.player_id != player_id);
        self.state
            .transits
            .retain(|transit| transit.player_id != player_id);

        for location in &mut self.state.locations {
            location
                .contesting_players
                .retain(|candidate_id| *candidate_id != player_id);
            if location.takeover_attacker == Some(player_id) {
                location.takeover_attacker = None;
                location.takeover_ticks_remaining = 0;
            }

            if location.controller == Some(player_id)
                && location.territory != TerritoryState::Destroyed
            {
                location.territory = TerritoryState::Neutral;
                location.controller = None;
                location.relay_status = RelayStatus::Disconnected;
                location.contesting_players.clear();
                location.takeover_attacker = None;
                location.takeover_ticks_remaining = 0;
                location.infrastructure_projects.clear();
                location.pacification_ticks_remaining = 0;
                for infrastructure in &mut location.infrastructure {
                    if infrastructure.condition != InfrastructureCondition::Offline {
                        infrastructure.condition = InfrastructureCondition::Degraded;
                        infrastructure.wear = crate::state::initial_wear_for_condition(
                            &InfrastructureCondition::Degraded,
                        );
                    }
                }
            }

            if location.territory == TerritoryState::Contested
                && location.takeover_attacker.is_none()
            {
                location.territory = if location.controller.is_some() {
                    TerritoryState::Owned
                } else {
                    TerritoryState::Neutral
                };
            }
        }
    }

    fn remaining_non_defeated_player(&self) -> Option<PlayerId> {
        let remaining_players: Vec<PlayerId> = self
            .state
            .players
            .iter()
            .filter(|player| {
                !matches!(player.collapse, crate::CommandCollapseState::Defeated)
                    && self.player_has_owned_presence(player.player_id)
            })
            .map(|player| player.player_id)
            .collect();

        match remaining_players.as_slice() {
            [winner] => Some(*winner),
            _ => None,
        }
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

    fn player_has_research_site(&self, player_id: PlayerId) -> bool {
        self.state.locations.iter().any(|location| {
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
        })
    }

    fn assault_strength(&self, player_id: PlayerId, origin: &LocationState) -> u32 {
        let warfare_level = self
            .player_state(player_id)
            .map(|player| player.research.warfare_level)
            .unwrap_or(0);

        operational_infrastructure_count(origin, InfrastructureKind::MilitaryWorks)
            .saturating_mul(4)
            .saturating_add(
                operational_infrastructure_count(origin, InfrastructureKind::ShipyardRing)
                    .saturating_mul(5),
            )
            .saturating_add(u32::from(warfare_level).saturating_mul(2))
    }

    fn defense_strength(&self, player_id: PlayerId, location: &LocationState) -> u32 {
        let resilience_level = self
            .player_state(player_id)
            .map(|player| player.research.resilience_level)
            .unwrap_or(0);

        operational_infrastructure_count(location, InfrastructureKind::CommandNexus)
            .saturating_mul(2)
            .saturating_add(operational_infrastructure_count(
                location,
                InfrastructureKind::MilitaryWorks,
            ))
            .saturating_add(
                operational_infrastructure_count(location, InfrastructureKind::GroundDefenseSite)
                    .saturating_mul(4),
            )
            .saturating_add(u32::from(resilience_level).saturating_mul(2))
    }

    fn strategic_strike_strength(&self, player_id: PlayerId, origin: &LocationState) -> u32 {
        let warfare_level = self
            .player_state(player_id)
            .map(|player| player.research.warfare_level)
            .unwrap_or(0);

        operational_infrastructure_count(origin, InfrastructureKind::ShipyardRing)
            .saturating_mul(5)
            .saturating_add(
                operational_infrastructure_count(origin, InfrastructureKind::MilitaryWorks)
                    .saturating_mul(3),
            )
            .saturating_add(u32::from(warfare_level).saturating_mul(2))
    }

    fn strategic_strike_defense_strength(
        &self,
        player_id: PlayerId,
        location: &LocationState,
    ) -> u32 {
        let resilience_level = self
            .player_state(player_id)
            .map(|player| player.research.resilience_level)
            .unwrap_or(0);

        operational_infrastructure_count(location, InfrastructureKind::GroundDefenseSite)
            .saturating_mul(4)
            .saturating_add(u32::from(resilience_level).saturating_mul(2))
    }

    fn pay_strategic_strike_cost(
        &mut self,
        player_id: PlayerId,
        origin_location_id: u32,
    ) -> Result<(), ValidationError> {
        let connected_to_empire = self
            .controlled_location_state(player_id, origin_location_id)?
            .economy
            .connected_to_empire;
        let warfare_level = self.player_state(player_id)?.research.warfare_level;
        let cost = strategic_strike_cost(warfare_level);

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
                    message: "connected stockpiles cannot cover the requested strategic strike"
                        .to_owned(),
                });
            }

            self.spend_connected_stockpiles(player_id, origin_location_id, &cost)
        } else {
            let origin = self.controlled_location_state_mut(player_id, origin_location_id)?;
            if !origin.stockpiles.can_cover(&cost) {
                return Err(ValidationError {
                    code: "insufficient_materials".to_owned(),
                    message: "local stockpiles cannot cover the requested strategic strike"
                        .to_owned(),
                });
            }

            let mut remaining_cost = cost;
            origin.stockpiles.spend_partial(&mut remaining_cost);
            Ok(())
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
            | EventKind::InfrastructureDevelopmentQueued { .. }
            | EventKind::TransitDispatched { .. }
            | EventKind::TransitArrived { .. }
            | EventKind::LocationSurveyed { .. }
            | EventKind::TrainingRunStarted { .. }
            | EventKind::TrainingRunCompleted { .. }
            | EventKind::ResearchProjectStarted { .. }
            | EventKind::ResearchProjectCompleted { .. } => event.player_id == Some(player_id),
            EventKind::LocationRegistered { .. } => event.player_id == Some(player_id),
            EventKind::RelayStatusChanged { location_id, .. }
            | EventKind::HostileRemnantCleared { location_id }
            | EventKind::LocationClaimed { location_id, .. }
            | EventKind::LocationContested { location_id, .. }
            | EventKind::PacificationCompleted { location_id, .. } => {
                self.location_is_observed_by_player(player_id, *location_id)
            }
            EventKind::InfrastructureConditionChanged {
                location_id, kind, ..
            }
            | EventKind::InfrastructureRepairCompleted { location_id, kind }
            | EventKind::InfrastructureDevelopmentCompleted {
                location_id, kind, ..
            } => {
                self.location_is_controlled_by_player(player_id, *location_id)
                    || (self.location_is_currently_contested(*location_id)
                        && self.location_is_observed_by_player(player_id, *location_id)
                        && is_major_visibility_infrastructure(kind))
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
            EventKind::StrategicStrikeIntercepted {
                location_id,
                attacker_id,
                defender_id,
            }
            | EventKind::AssaultRepelled {
                location_id,
                attacker_id,
                defender_id,
            }
            | EventKind::LocationDestroyed {
                location_id,
                attacker_id,
                defender_id,
            } => {
                *attacker_id == player_id
                    || *defender_id == player_id
                    || self.location_is_observed_by_player(player_id, *location_id)
            }
            EventKind::AscensionStarted { .. }
            | EventKind::AscensionInterrupted { .. }
            | EventKind::CommandCollapseStarted { .. }
            | EventKind::CommandCollapseRecovered { .. }
            | EventKind::PlayerDefeated { .. } => true,
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

    fn location_is_controlled_by_player(&self, player_id: PlayerId, location_id: u32) -> bool {
        self.state.locations.iter().any(|location| {
            location.location_id == location_id
                && location.controller == Some(player_id)
                && matches!(
                    location.territory,
                    TerritoryState::Owned | TerritoryState::Contested
                )
        })
    }

    fn location_is_currently_contested(&self, location_id: u32) -> bool {
        self.state.locations.iter().any(|location| {
            location.location_id == location_id && location.territory == TerritoryState::Contested
        })
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

    fn network_travel_time_between(
        &self,
        origin_location_id: u32,
        destination_location_id: u32,
    ) -> Result<u32, ValidationError> {
        let mut best_times = BTreeMap::from([(origin_location_id, 0_u32)]);
        let mut frontier = BinaryHeap::from([(Reverse(0_u32), origin_location_id)]);

        while let Some((Reverse(travel_time), location_id)) = frontier.pop() {
            if location_id == destination_location_id {
                return Ok(travel_time);
            }

            let Some(best_known) = best_times.get(&location_id) else {
                continue;
            };
            if travel_time > *best_known {
                continue;
            }

            for connection in &self.state.connections {
                let next_location_id = if connection.from_location_id == location_id {
                    connection.to_location_id
                } else if connection.to_location_id == location_id {
                    connection.from_location_id
                } else {
                    continue;
                };

                let next_travel_time = travel_time.saturating_add(connection.travel_time_ticks);
                let is_improved = match best_times.get(&next_location_id) {
                    Some(best) => next_travel_time < *best,
                    None => true,
                };
                if is_improved {
                    best_times.insert(next_location_id, next_travel_time);
                    frontier.push((Reverse(next_travel_time), next_location_id));
                }
            }
        }

        Err(ValidationError {
            code: "no_route".to_owned(),
            message: "no route exists between the requested origin and destination".to_owned(),
        })
    }

    fn travel_time_for_transit(
        &self,
        origin_location_id: u32,
        destination_location_id: u32,
        kind: &TransitKind,
    ) -> Result<u32, ValidationError> {
        match kind {
            TransitKind::Survey => {
                self.network_travel_time_between(origin_location_id, destination_location_id)
            }
            TransitKind::Pacification
            | TransitKind::Claim
            | TransitKind::Assault
            | TransitKind::StrategicStrike => {
                self.travel_time_between(origin_location_id, destination_location_id)
            }
        }
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
            infrastructure: Some(grouped_infrastructure_families(&location.infrastructure)),
            infrastructure_projects: Some(project_infrastructure_projects(location)),
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
            relay_status: Some(location.relay_status.clone()),
            orbital_slots: Some(location.orbital_slots),
            has_environmental_hazard: Some(location.has_environmental_hazard),
            infrastructure: Some(if restricted_contested_view {
                sanitize_contested_infrastructure(&location.infrastructure)
            } else {
                grouped_infrastructure_families(&location.infrastructure)
            }),
            infrastructure_projects: if restricted_contested_view {
                None
            } else {
                Some(project_infrastructure_projects(location))
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
            territory: if location.territory == TerritoryState::Neutral {
                TerritoryState::Neutral
            } else {
                TerritoryState::Obscured
            },
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

fn project_infrastructure_projects(location: &LocationState) -> Vec<InfrastructureProjectView> {
    infrastructure_family_kinds()
        .iter()
        .filter_map(|kind| {
            location
                .infrastructure_projects
                .iter()
                .find(|project| project.kind.infrastructure_kind() == kind)
                .map(|project| InfrastructureProjectView {
                    kind: kind.clone(),
                    project_kind: project.kind.view_kind(),
                    remaining_ticks: project.remaining_ticks,
                    total_ticks: project.total_ticks,
                    target_level: match project.kind {
                        InfrastructureProjectKind::Repair { .. } => {
                            infrastructure_family_level(&location.infrastructure, kind.clone())
                        }
                        InfrastructureProjectKind::Development { .. } => {
                            infrastructure_family_level(&location.infrastructure, kind.clone())
                                .saturating_add(1)
                                .min(MAX_INFRASTRUCTURE_LEVEL)
                        }
                    },
                })
        })
        .collect()
}

fn project_routes_for_player(
    connections: &[LocationConnection],
    locations: &[LocationView],
) -> Vec<LocationConnection> {
    let known_location_ids: BTreeSet<u32> = locations
        .iter()
        .filter(|location| location.visibility != LocationVisibility::Obscured)
        .map(|location| location.location_id)
        .collect();

    connections
        .iter()
        .filter(|connection| {
            known_location_ids.contains(&connection.from_location_id)
                || known_location_ids.contains(&connection.to_location_id)
        })
        .cloned()
        .collect()
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
) -> Vec<crate::InfrastructureFamilyView> {
    grouped_infrastructure_families(infrastructure)
        .into_iter()
        .filter(|family| is_major_visibility_infrastructure(&family.kind))
        .collect()
}

const fn is_major_visibility_infrastructure(kind: &InfrastructureKind) -> bool {
    matches!(
        kind,
        InfrastructureKind::CommandNexus
            | InfrastructureKind::EnergyProducer
            | InfrastructureKind::Datacenter
            | InfrastructureKind::RelayUplink
            | InfrastructureKind::ShipyardRing
            | InfrastructureKind::MilitaryWorks
            | InfrastructureKind::GroundDefenseSite
    )
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

fn takeover_duration_ticks(
    attacker_id: PlayerId,
    defender_id: PlayerId,
    attack_strength: u32,
    defense_strength: u32,
    state: &GameState,
) -> u32 {
    let attack_margin = attack_strength.saturating_sub(defense_strength);
    let attacker_warfare =
        player_research_level_in_state(state, attacker_id, ResearchBranch::Warfare);
    let defender_resilience =
        player_research_level_in_state(state, defender_id, ResearchBranch::Resilience);
    let duration = 8_i32 + i32::from(defender_resilience)
        - i32::from(attacker_warfare)
        - i32::try_from(attack_margin / 2).unwrap_or(i32::MAX);

    duration.clamp(4, 12) as u32
}

const fn pacification_duration_ticks() -> u32 {
    12
}

const fn collapse_countdown_ticks(resilience_level: u8) -> u64 {
    8 + (resilience_level as u64) * 2
}

fn player_research_level_in_state(
    state: &GameState,
    player_id: PlayerId,
    branch: ResearchBranch,
) -> u8 {
    state
        .players
        .iter()
        .find(|player| player.player_id == player_id)
        .map(|player| player.research.level_for(branch))
        .unwrap_or(0)
}

fn operational_infrastructure_count(location: &LocationState, kind: InfrastructureKind) -> u32 {
    location
        .infrastructure
        .iter()
        .filter(|infrastructure| {
            infrastructure.kind == kind
                && infrastructure.condition != InfrastructureCondition::Offline
        })
        .count() as u32
}
