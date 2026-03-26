use crate::{
    BuildCapacity, CommandEnvelope, CommandKind, EnergyPotential, EventKind, EventRecord,
    GameConfig, GameState, InfrastructureCondition, InfrastructureKind, InfrastructureProjectKind,
    InfrastructureProjectState, LocationKind, LocationState, PlayerId, RelayStatus, ReplayLog,
    ResourceRichness, ResourceStockpiles, ScenarioConfig, SessionId, Snapshot, StrategicPosition,
    TerritoryState, ValidationError,
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

        let completions = self.state.advance_infrastructure_projects();
        if !completions.is_empty() {
            for completion in completions {
                let kind = match completion.project_kind {
                    InfrastructureProjectKind::Repair { .. } => {
                        EventKind::InfrastructureRepairCompleted {
                            location_id: completion.location_id,
                            kind: completion.kind,
                        }
                    }
                    InfrastructureProjectKind::Construction { .. } => {
                        EventKind::InfrastructureConstructionCompleted {
                            location_id: completion.location_id,
                            kind: completion.kind,
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
            } => self.apply_set_relay_status(location_id, relay_status),
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
        });
        self.state
            .locations
            .sort_by_key(|location| location.location_id);
        self.state.recompute_economy();

        Ok(vec![EventKind::LocationRegistered { location_id, name }])
    }

    fn apply_set_relay_status(
        &mut self,
        location_id: u32,
        relay_status: RelayStatus,
    ) -> Result<Vec<EventKind>, ValidationError> {
        let location = self.location_state_mut(location_id)?;
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
        let (connected_to_empire, build_capacity, has_environmental_hazard, condition) = {
            let location = self.controlled_location_state(player_id, location_id)?;
            if location.infrastructure_projects.iter().any(|project| {
                matches!(
                    project.kind,
                    InfrastructureProjectKind::Repair {
                        infrastructure_kind: ref queued_kind,
                    } if *queued_kind == infrastructure_kind
                )
            }) {
                return Err(ValidationError {
                    code: "repair_already_queued".to_owned(),
                    message: "a repair project is already queued for this infrastructure"
                        .to_owned(),
                });
            }

            let infrastructure = location
                .infrastructure
                .iter()
                .find(|infrastructure| infrastructure.kind == infrastructure_kind)
                .ok_or(ValidationError {
                    code: "missing_infrastructure".to_owned(),
                    message: "repair command references infrastructure that is not present at the location"
                        .to_owned(),
                })?;

            if infrastructure.condition == InfrastructureCondition::Operational {
                return Err(ValidationError {
                    code: "infrastructure_not_damaged".to_owned(),
                    message: "repair can only be queued for degraded or offline infrastructure"
                        .to_owned(),
                });
            }

            (
                location.economy.connected_to_empire,
                location.build_capacity.clone(),
                location.has_environmental_hazard,
                infrastructure.condition.clone(),
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
