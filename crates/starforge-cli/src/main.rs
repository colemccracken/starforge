use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;
use starforge_core::{
    CommandKind, GameSession, InfrastructureCondition, LocationView, PlayerId, SessionId, TickId,
    TransitKind, VictoryState,
};
use starforge_scenarios::starter_skirmish_harness;

use crate::cli::{Cli, Command as CliCommand};

mod cli;

type DynError = Box<dyn Error>;

fn main() {
    let cli = Cli::try_parse().unwrap_or_else(|error| error.exit());

    if let Err(error) = run(cli) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), DynError> {
    match cli.command {
        CliCommand::New(args) => cmd_new(args.session),
        CliCommand::Status(args) => cmd_status(&args.session.session, args.player),
        CliCommand::Map(args) => cmd_map(&args.session.session, args.player),
        CliCommand::Events(args) => cmd_events(
            &args.common.session.session,
            args.common.player,
            TickId::new(args.from_tick),
        ),
        CliCommand::Step(args) => cmd_step(&args.session.session, args.ticks),
        CliCommand::Survey(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchSurveyTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "survey expedition queued",
        ),
        CliCommand::Pacify(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchPacificationTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "pacification expedition queued",
        ),
        CliCommand::Claim(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchClaimTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "claim expedition queued",
        ),
        CliCommand::Build(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::QueueInfrastructureConstruction {
                location_id: args.location,
                infrastructure_kind: args.kind,
            },
            "construction queued",
        ),
        CliCommand::Repair(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::QueueInfrastructureRepair {
                location_id: args.location,
                infrastructure_kind: args.kind,
            },
            "repair queued",
        ),
        CliCommand::Relay(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::SetRelayStatus {
                location_id: args.location,
                relay_status: args.status,
            },
            "relay status updated",
        ),
        CliCommand::Budget(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: args.upkeep,
                reserved_for_training: args.training,
                reserved_for_agents: args.agents,
            },
            "throughput budget updated",
        ),
        CliCommand::Train(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::StartTrainingRun {
                target_tier: args.target_tier,
            },
            "training run started",
        ),
    }
}

fn cmd_new(session_path: Option<PathBuf>) -> Result<(), DynError> {
    let session_path = session_path.unwrap_or_else(default_session_path);
    if session_path.exists() {
        return Err(format!("session file '{}' already exists", session_path.display()).into());
    }

    let harness = starter_skirmish_harness()?;
    let session = harness.instantiate_session(SessionId::new(1));
    save_session(&session_path, &session)?;

    println!("Created session at {}", session_path.display());
    println!("Scenario: {}", harness.name);
    println!("Players:");
    for location in session
        .state()
        .locations
        .iter()
        .filter(|location| location.homeworld_of.is_some())
    {
        let player_id = location
            .homeworld_of
            .expect("homeworld should map to a player");
        println!(
            "  P{} homeworld: {} (# {})",
            player_id.0, location.name, location.location_id
        );
    }
    println!();
    println!("Suggested first steps:");
    println!(
        "  starforge-cli map --session {} --player 1",
        session_path.display()
    );
    println!(
        "  starforge-cli status --session {} --player 1",
        session_path.display()
    );
    if let Some((origin_id, destination_id, travel_time)) =
        first_recommended_route(&session, PlayerId::new(1))
    {
        println!(
            "  starforge-cli survey --session {} --player 1 --origin {} --destination {}",
            session_path.display(),
            origin_id,
            destination_id
        );
        println!(
            "  starforge-cli step --session {} --ticks {}",
            session_path.display(),
            travel_time
        );
    }
    Ok(())
}

fn cmd_status(session_path: &Path, player_id: PlayerId) -> Result<(), DynError> {
    let session = load_session(session_path)?;
    let view = session.player_view(player_id)?;
    let owned_count = view
        .locations
        .iter()
        .filter(|location| location.visibility == starforge_core::LocationVisibility::Owned)
        .count();

    println!("Session: {}", session_path.display());
    println!("Tick: {}", view.tick_id.0);
    println!("Victory: {}", render_victory(&session.state().victory));
    println!("Player: P{}", player_id.0);
    println!("Model tier: {}", view.model_tier);
    println!("Owned worlds: {}", owned_count);
    println!(
        "Throughput: available={} upkeep={} training={} agents={}",
        view.throughput.available,
        view.throughput.reserved_for_model_upkeep,
        view.throughput.reserved_for_training,
        view.throughput.reserved_for_agents
    );
    println!(
        "Connected economy: energy={} datacenter={} usable_throughput={}",
        view.economy.total_connected_energy,
        view.economy.total_connected_datacenter_capacity,
        view.economy.usable_throughput
    );
    println!(
        "Connected stockpiles: {}",
        format_stockpiles(&view.economy.connected_stockpiles)
    );

    match &view.training {
        Some(training) => println!(
            "Training: tier {} progress {}/{} requiring {} training throughput",
            training.target_tier,
            training.progress_ticks,
            training.required_ticks,
            training.required_training_throughput
        ),
        None => println!("Training: none"),
    }

    if view.transits.is_empty() {
        println!("Transits: none");
    } else {
        println!("Transits:");
        for transit in &view.transits {
            println!(
                "  #{} {} {} -> {} eta={}",
                transit.transit_id,
                render_transit_kind(&transit.kind),
                transit.origin_id,
                transit.destination_id,
                transit.eta_tick.0
            );
        }
    }

    Ok(())
}

fn cmd_map(session_path: &Path, player_id: PlayerId) -> Result<(), DynError> {
    let session = load_session(session_path)?;
    let view = session.player_view(player_id)?;

    println!(
        "Map for P{} at tick {} ({})",
        player_id.0,
        view.tick_id.0,
        render_victory(&session.state().victory)
    );
    for location in &view.locations {
        println!("{}", render_location(location));
    }
    println!();
    println!("Reachable routes from currently known worlds:");
    for route in render_known_routes(&session, &view.locations) {
        println!("  {route}");
    }

    Ok(())
}

fn cmd_events(session_path: &Path, player_id: PlayerId, from_tick: TickId) -> Result<(), DynError> {
    let session = load_session(session_path)?;
    let events = session.player_events(player_id, from_tick)?;

    if events.is_empty() {
        println!(
            "No visible events for P{} from tick {}",
            player_id.0, from_tick.0
        );
        return Ok(());
    }

    for event in events {
        println!("[{}] {}", event.tick_id.0, render_event(&event.kind));
    }

    Ok(())
}

fn cmd_step(session_path: &Path, ticks: u32) -> Result<(), DynError> {
    let mut session = load_session(session_path)?;
    session.advance_ticks(ticks);
    save_session(session_path, &session)?;
    println!(
        "Advanced to tick {}. {}",
        session.current_tick().0,
        render_victory(&session.state().victory)
    );
    Ok(())
}

fn cmd_mutate(
    session_path: &Path,
    player_id: PlayerId,
    command: CommandKind,
    success_message: &str,
) -> Result<(), DynError> {
    let mut session = load_session(session_path)?;
    session.issue_command_now(player_id, command)?;
    save_session(session_path, &session)?;
    println!("{success_message} at tick {}", session.current_tick().0);
    Ok(())
}

fn load_session(session_path: &Path) -> Result<GameSession, DynError> {
    let json = fs::read_to_string(session_path)?;
    Ok(GameSession::from_snapshot_json(&json)?)
}

fn save_session(session_path: &Path, session: &GameSession) -> Result<(), DynError> {
    if let Some(parent) = session_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(session_path, session.snapshot_json()?)?;
    Ok(())
}

fn default_session_path() -> PathBuf {
    PathBuf::from("starforge-session.json")
}

fn first_recommended_route(session: &GameSession, player_id: PlayerId) -> Option<(u32, u32, u32)> {
    let homeworld_id = session
        .state()
        .locations
        .iter()
        .find(|location| location.homeworld_of == Some(player_id))
        .map(|location| location.location_id)?;

    let mut fallback = None;
    for connection in &session.state().connections {
        let route = if connection.from_location_id == homeworld_id {
            Some((
                connection.from_location_id,
                connection.to_location_id,
                connection.travel_time_ticks,
            ))
        } else if connection.to_location_id == homeworld_id {
            Some((
                connection.to_location_id,
                connection.from_location_id,
                connection.travel_time_ticks,
            ))
        } else {
            None
        };

        let Some(route) = route else {
            continue;
        };
        if fallback.is_none() {
            fallback = Some(route);
        }

        let destination = session
            .state()
            .locations
            .iter()
            .find(|location| location.location_id == route.1)?;
        if destination.controller.is_none() && destination.homeworld_of.is_none() {
            return Some(route);
        }
    }

    fallback
}

fn render_known_routes(session: &GameSession, locations: &[LocationView]) -> Vec<String> {
    let known_location_ids: Vec<u32> = locations
        .iter()
        .filter(|location| location.visibility != starforge_core::LocationVisibility::Obscured)
        .map(|location| location.location_id)
        .collect();

    session
        .state()
        .connections
        .iter()
        .filter(|connection| {
            known_location_ids.contains(&connection.from_location_id)
                || known_location_ids.contains(&connection.to_location_id)
        })
        .map(|connection| {
            format!(
                "{} <-> {} (eta {})",
                connection.from_location_id,
                connection.to_location_id,
                connection.travel_time_ticks
            )
        })
        .collect()
}

fn render_location(location: &LocationView) -> String {
    let visibility = format!("{:?}", location.visibility);
    let territory = format!("{:?}", location.territory);
    let mut summary = format!(
        "#{:>2} {:<20} {:<9} territory={}",
        location.location_id, location.name, visibility, territory,
    );

    if let Some(controller) = location.controller {
        summary.push_str(&format!(" controller=P{}", controller.0));
    }
    if let Some(kind) = &location.kind {
        summary.push_str(&format!(" kind={kind:?}"));
    }
    if let Some(resource_richness) = &location.resource_richness {
        summary.push_str(&format!(" resources={resource_richness:?}"));
    }
    if let Some(energy_potential) = &location.energy_potential {
        summary.push_str(&format!(" energy_potential={energy_potential:?}"));
    }
    if let Some(build_capacity) = &location.build_capacity {
        summary.push_str(&format!(" build={build_capacity:?}"));
    }
    if let Some(has_environmental_hazard) = location.has_environmental_hazard {
        summary.push_str(&format!(" hazard={has_environmental_hazard}"));
    }
    if let Some(hostile_remnant_present) = location.hostile_remnant_present {
        summary.push_str(&format!(" remnant={hostile_remnant_present}"));
    }
    if let Some(relay_status) = &location.relay_status {
        summary.push_str(&format!(" relay={relay_status:?}"));
    }
    if let Some(infrastructure) = &location.infrastructure {
        summary.push_str(&format!(
            " infra=[{}]",
            infrastructure
                .iter()
                .map(render_infrastructure)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(projects) = &location.infrastructure_projects
        && !projects.is_empty()
    {
        summary.push_str(&format!(" projects={projects:?}"));
    }
    if let Some(economy) = &location.economy {
        summary.push_str(&format!(
            " economy=(energy={} dc={} throughput={} connected={})",
            economy.generated_energy,
            economy.datacenter_capacity,
            economy.empire_usable_throughput,
            economy.connected_to_empire
        ));
    }
    if let Some(stockpiles) = &location.stockpiles {
        summary.push_str(&format!(" stockpiles={}", format_stockpiles(stockpiles)));
    }

    summary
}

fn render_infrastructure(infrastructure: &starforge_core::InfrastructureState) -> String {
    match infrastructure.condition {
        InfrastructureCondition::Operational => format!("{:?}", infrastructure.kind),
        _ => format!("{:?}({:?})", infrastructure.kind, infrastructure.condition),
    }
}

fn format_stockpiles(stockpiles: &starforge_core::ResourceStockpiles) -> String {
    format!(
        "common={} volatiles={} rare={}",
        stockpiles.common_materials, stockpiles.volatiles, stockpiles.rare_materials
    )
}

fn render_victory(victory: &VictoryState) -> String {
    match victory {
        VictoryState::Ongoing => "victory=ongoing".to_owned(),
        VictoryState::Won { winner } => format!("victory=won by P{}", winner.0),
    }
}

fn render_transit_kind(kind: &TransitKind) -> &'static str {
    match kind {
        TransitKind::Survey => "survey",
        TransitKind::Pacification => "pacify",
        TransitKind::Claim => "claim",
    }
}

fn render_event(event: &starforge_core::EventKind) -> String {
    match event {
        starforge_core::EventKind::SessionCreated { player_ids, seed } => {
            format!(
                "session created for players {:?} seed={}",
                player_ids, seed.0
            )
        }
        starforge_core::EventKind::TickAdvanced { tick_id } => {
            format!("tick advanced to {}", tick_id.0)
        }
        starforge_core::EventKind::CommandAccepted {
            command,
            apply_at_tick,
        } => format!(
            "command accepted {:?} apply_at={}",
            command, apply_at_tick.0
        ),
        starforge_core::EventKind::CommandApplied { command } => {
            format!("command applied {:?}", command)
        }
        starforge_core::EventKind::CommandRejected { command, error } => {
            format!(
                "command rejected {:?}: {} ({})",
                command, error.message, error.code
            )
        }
        starforge_core::EventKind::ThroughputBudgetSet {
            reserved_for_model_upkeep,
            reserved_for_training,
            reserved_for_agents,
            available,
        } => format!(
            "throughput budget upkeep={} training={} agents={} available={}",
            reserved_for_model_upkeep, reserved_for_training, reserved_for_agents, available
        ),
        starforge_core::EventKind::EconomyUpdated {
            player_id,
            total_connected_energy,
            total_connected_datacenter_capacity,
            usable_throughput,
        } => format!(
            "economy updated for P{} energy={} datacenter={} throughput={}",
            player_id.0,
            total_connected_energy,
            total_connected_datacenter_capacity,
            usable_throughput
        ),
        starforge_core::EventKind::AgentAssigned {
            role,
            scope,
            reserved_throughput,
        } => format!(
            "agent assigned role={} scope={} throughput={}",
            role, scope, reserved_throughput
        ),
        starforge_core::EventKind::LocationRegistered { location_id, name } => {
            format!("location registered #{} {}", location_id, name)
        }
        starforge_core::EventKind::RelayStatusChanged {
            location_id,
            relay_status,
        } => format!(
            "relay status changed at #{} to {:?}",
            location_id, relay_status
        ),
        starforge_core::EventKind::InfrastructureConditionChanged {
            location_id,
            kind,
            condition,
        } => format!(
            "infrastructure condition changed at #{} {:?} -> {:?}",
            location_id, kind, condition
        ),
        starforge_core::EventKind::InfrastructureRepairQueued {
            location_id,
            kind,
            duration_ticks,
            cost,
        } => format!(
            "repair queued at #{} {:?} duration={} cost={}",
            location_id,
            kind,
            duration_ticks,
            format_stockpiles(cost)
        ),
        starforge_core::EventKind::InfrastructureRepairCompleted { location_id, kind } => {
            format!("repair completed at #{} {:?}", location_id, kind)
        }
        starforge_core::EventKind::InfrastructureConstructionQueued {
            location_id,
            kind,
            duration_ticks,
            cost,
        } => format!(
            "construction queued at #{} {:?} duration={} cost={}",
            location_id,
            kind,
            duration_ticks,
            format_stockpiles(cost)
        ),
        starforge_core::EventKind::InfrastructureConstructionCompleted { location_id, kind } => {
            format!("construction completed at #{} {:?}", location_id, kind)
        }
        starforge_core::EventKind::TransitDispatched {
            transit_id,
            origin_id,
            destination_id,
            eta_tick,
            kind,
        } => format!(
            "transit #{} {} {} -> {} eta={}",
            transit_id,
            render_transit_kind(kind),
            origin_id,
            destination_id,
            eta_tick.0
        ),
        starforge_core::EventKind::TransitArrived {
            transit_id,
            destination_id,
            kind,
        } => format!(
            "transit #{} {} arrived at {}",
            transit_id,
            render_transit_kind(kind),
            destination_id
        ),
        starforge_core::EventKind::LocationSurveyed { location_id } => {
            format!("location #{} surveyed", location_id)
        }
        starforge_core::EventKind::HostileRemnantCleared { location_id } => {
            format!("hostile remnant cleared at #{}", location_id)
        }
        starforge_core::EventKind::LocationClaimed {
            location_id,
            player_id,
        } => format!("location #{} claimed by P{}", location_id, player_id.0),
        starforge_core::EventKind::TrainingRunStarted {
            target_tier,
            required_training_throughput,
            required_ticks,
        } => format!(
            "training run started for tier {} requiring {} throughput for {} ticks",
            target_tier, required_training_throughput, required_ticks
        ),
        starforge_core::EventKind::TrainingRunCompleted { achieved_tier } => {
            format!("training run completed; achieved tier {}", achieved_tier)
        }
        starforge_core::EventKind::VictoryDeclared { winner, reason } => {
            format!("victory declared for P{} ({})", winner.0, reason)
        }
    }
}
