use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;
use reqwest::blocking::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;
use starforge_api::{
    ApiSessionSummary, IssueCommandRequest, SaveSessionResponse, SessionControlState,
    SessionMetrics, StepSessionRequest,
};
use starforge_core::{
    CommandCollapseState, CommandKind, EventRecord, GameSession, InfrastructureCondition,
    LocationConnection, LocationView, PlayerId, PlayerStateView, ResearchBranch, SessionId, TickId,
    TransitKind, VictoryState,
};
use starforge_scenarios::{
    ScenarioHarness, default_ruleset_path, default_scenario_path, load_harness,
    starter_skirmish_harness,
};

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
    let Cli { api_base, command } = cli;
    match api_base.as_deref() {
        Some(api_base) => run_api(api_base, command),
        None => run_file(command),
    }
}

fn run_file(command: CliCommand) -> Result<(), DynError> {
    match command {
        CliCommand::New(args) => cmd_new(args.session),
        CliCommand::Status(args) => cmd_status(&args.session.session, args.player),
        CliCommand::Map(args) => cmd_map(&args.session.session, args.player),
        CliCommand::Events(args) => cmd_events(
            &args.common.session.session,
            args.common.player,
            TickId::new(args.from_tick),
        ),
        CliCommand::Metrics(args) => cmd_metrics(&args.session),
        CliCommand::Save(args) => cmd_save(&args.session.session, args.output.as_deref()),
        CliCommand::Load(args) => cmd_load(&args.input, args.session.as_deref()),
        CliCommand::ScenarioRun(args) => cmd_scenario_run(
            args.ruleset.as_deref(),
            args.scenario.as_deref(),
            args.session,
            args.ticks,
        ),
        CliCommand::Run(args) => cmd_run(&args.session),
        CliCommand::Pause(args) => cmd_pause(&args.session),
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
        CliCommand::Assault(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchAssaultTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "assault expedition queued",
        ),
        CliCommand::Strike(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchStrategicStrike {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "strategic strike queued",
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
                reserved_for_research: args.research,
                reserved_for_training: args.training,
                reserved_for_agents: args.agents,
            },
            "throughput budget updated",
        ),
        CliCommand::Research(args) => cmd_mutate(
            &args.common.session.session,
            args.common.player,
            CommandKind::StartResearchProject {
                branch: args.branch,
                target_level: args.target_level,
            },
            "research project started",
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

fn run_api(api_base: &str, command: CliCommand) -> Result<(), DynError> {
    match command {
        CliCommand::New(args) => cmd_new_api(api_base, args.session),
        CliCommand::Status(args) => cmd_status_api(api_base, &args.session.session, args.player),
        CliCommand::Map(args) => cmd_map_api(api_base, &args.session.session, args.player),
        CliCommand::Events(args) => cmd_events_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            TickId::new(args.from_tick),
        ),
        CliCommand::Metrics(args) => cmd_metrics_api(api_base, &args.session),
        CliCommand::Save(args) => {
            cmd_save_api(api_base, &args.session.session, args.output.as_deref())
        }
        CliCommand::Load(args) => cmd_load_api(api_base, &args.input, args.session.as_deref()),
        CliCommand::ScenarioRun(args) => cmd_scenario_run_api(
            api_base,
            args.ruleset.as_deref(),
            args.scenario.as_deref(),
            args.session,
            args.ticks,
        ),
        CliCommand::Run(args) => cmd_run_api(api_base, &args.session),
        CliCommand::Pause(args) => cmd_pause_api(api_base, &args.session),
        CliCommand::Step(args) => cmd_step_api(api_base, &args.session.session, args.ticks),
        CliCommand::Survey(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchSurveyTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "survey expedition queued",
        ),
        CliCommand::Pacify(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchPacificationTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "pacification expedition queued",
        ),
        CliCommand::Claim(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchClaimTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "claim expedition queued",
        ),
        CliCommand::Assault(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchAssaultTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "assault expedition queued",
        ),
        CliCommand::Strike(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::DispatchStrategicStrike {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            },
            "strategic strike queued",
        ),
        CliCommand::Build(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::QueueInfrastructureConstruction {
                location_id: args.location,
                infrastructure_kind: args.kind,
            },
            "construction queued",
        ),
        CliCommand::Repair(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::QueueInfrastructureRepair {
                location_id: args.location,
                infrastructure_kind: args.kind,
            },
            "repair queued",
        ),
        CliCommand::Relay(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::SetRelayStatus {
                location_id: args.location,
                relay_status: args.status,
            },
            "relay status updated",
        ),
        CliCommand::Budget(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: args.upkeep,
                reserved_for_research: args.research,
                reserved_for_training: args.training,
                reserved_for_agents: args.agents,
            },
            "throughput budget updated",
        ),
        CliCommand::Research(args) => cmd_mutate_api(
            api_base,
            &args.common.session.session,
            args.common.player,
            CommandKind::StartResearchProject {
                branch: args.branch,
                target_level: args.target_level,
            },
            "research project started",
        ),
        CliCommand::Train(args) => cmd_mutate_api(
            api_base,
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
    print_created_session(&session_path, &harness, &session, "Created session at");
    Ok(())
}

fn cmd_status(session_path: &Path, player_id: PlayerId) -> Result<(), DynError> {
    let session = load_session(session_path)?;
    let view = session.player_view(player_id)?;
    print_status(
        &session_path.display().to_string(),
        None,
        &session.state().victory,
        player_id,
        &view,
    );
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
    for route in render_known_routes(&view.routes) {
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

fn cmd_metrics(session_path: &Path) -> Result<(), DynError> {
    let session = load_session(session_path)?;
    let snapshot = session.snapshot();
    let metrics = SessionMetrics {
        session_id: session.session_id(),
        current_tick: session.current_tick(),
        control_state: SessionControlState::Paused,
        event_count: session.event_log().len(),
        accepted_command_count: session.replay_log().accepted_commands.len(),
        pending_command_count: snapshot.pending_commands.len(),
        transit_count: session.state().transits.len(),
    };
    print_metrics(&session_path.display().to_string(), &metrics);
    Ok(())
}

fn cmd_save(session_path: &Path, output_path: Option<&Path>) -> Result<(), DynError> {
    let session = load_session(session_path)?;
    let output_path = output_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_snapshot_output_path(session_path));
    write_snapshot_json(&output_path, &session.snapshot_json()?)?;
    println!(
        "Saved snapshot for session {} to {}",
        session.session_id().0,
        output_path.display()
    );
    Ok(())
}

fn cmd_load(input_path: &Path, session_path: Option<&Path>) -> Result<(), DynError> {
    let snapshot_json = fs::read_to_string(input_path)?;
    let session = GameSession::from_snapshot_json(&snapshot_json)?;
    let session_path = session_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_session_path);

    if session_path.exists() {
        return Err(format!("session file '{}' already exists", session_path.display()).into());
    }

    save_session(&session_path, &session)?;
    println!(
        "Loaded session {} from {} into {}",
        session.session_id().0,
        input_path.display(),
        session_path.display()
    );
    Ok(())
}

fn cmd_scenario_run(
    ruleset_path: Option<&Path>,
    scenario_path: Option<&Path>,
    session_path: Option<PathBuf>,
    ticks: u32,
) -> Result<(), DynError> {
    let harness = load_cli_harness(ruleset_path, scenario_path)?;
    let session_path = session_path.unwrap_or_else(default_session_path);
    if session_path.exists() {
        return Err(format!("session file '{}' already exists", session_path.display()).into());
    }

    let mut session = harness.instantiate_session(SessionId::new(1));
    if ticks > 0 {
        session.advance_ticks(ticks);
    }
    save_session(&session_path, &session)?;

    print_created_session(
        &session_path,
        &harness,
        &session,
        "Created scenario session at",
    );
    if ticks > 0 {
        println!(
            "Advanced scenario session to tick {} during setup.",
            session.current_tick().0
        );
    }
    Ok(())
}

fn cmd_run(session_path: &Path) -> Result<(), DynError> {
    let _ = load_session(session_path)?;
    Err(format!(
        "continuous run is only available in API mode; use `step --session {} --ticks <N>` for file-backed sessions",
        session_path.display()
    )
    .into())
}

fn cmd_pause(session_path: &Path) -> Result<(), DynError> {
    let _ = load_session(session_path)?;
    Err(format!(
        "continuous run is only available in API mode; file-backed session {} advances only when stepped explicitly",
        session_path.display()
    )
    .into())
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

fn cmd_new_api(api_base: &str, session_path: Option<PathBuf>) -> Result<(), DynError> {
    if let Some(session_path) = session_path {
        return Err(format!(
            "api mode does not use session files; remove --session {} and rerun",
            session_path.display()
        )
        .into());
    }

    let client = api_client();
    let summary: ApiSessionSummary = api_post_empty(
        &client,
        &format!("{}/sessions", normalize_api_base(api_base)),
    )?;

    println!(
        "Created remote session #{} via {}",
        summary.session_id.0,
        normalize_api_base(api_base)
    );
    println!("Scenario: {}", summary.scenario_name);
    println!();
    println!("Suggested first steps:");
    println!(
        "  starforge-cli --api-base {} map --session {} --player 1",
        normalize_api_base(api_base),
        summary.session_id.0
    );
    println!(
        "  starforge-cli --api-base {} status --session {} --player 1",
        normalize_api_base(api_base),
        summary.session_id.0
    );

    Ok(())
}

fn cmd_status_api(api_base: &str, session_arg: &Path, player_id: PlayerId) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary = api_session_summary(&client, api_base, session_id)?;
    let view = api_player_view(&client, api_base, session_id, player_id)?;
    print_status(
        &format!("#{} @ {}", session_id.0, normalize_api_base(api_base)),
        Some(summary.control_state),
        &summary.victory,
        player_id,
        &view,
    );
    Ok(())
}

fn cmd_map_api(api_base: &str, session_arg: &Path, player_id: PlayerId) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary = api_session_summary(&client, api_base, session_id)?;
    let view = api_player_view(&client, api_base, session_id, player_id)?;

    println!(
        "Map for P{} at tick {} ({})",
        player_id.0,
        view.tick_id.0,
        render_victory(&summary.victory)
    );
    for location in &view.locations {
        println!("{}", render_location(location));
    }
    println!();
    println!("Reachable routes from currently known worlds:");
    for route in render_known_routes(&view.routes) {
        println!("  {route}");
    }
    Ok(())
}

fn cmd_events_api(
    api_base: &str,
    session_arg: &Path,
    player_id: PlayerId,
    from_tick: TickId,
) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let events = api_player_events(&client, api_base, session_id, player_id, from_tick)?;

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

fn cmd_metrics_api(api_base: &str, session_arg: &Path) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let metrics: SessionMetrics = api_get_json(
        &client,
        &format!(
            "{}/sessions/{}/metrics",
            normalize_api_base(api_base),
            session_id.0
        ),
    )?;
    print_metrics(
        &format!("#{} @ {}", session_id.0, normalize_api_base(api_base)),
        &metrics,
    );
    Ok(())
}

fn cmd_save_api(
    api_base: &str,
    session_arg: &Path,
    output_path: Option<&Path>,
) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let saved: SaveSessionResponse = api_post_empty(
        &client,
        &format!(
            "{}/sessions/{}/save",
            normalize_api_base(api_base),
            session_id.0
        ),
    )?;
    let output_path = output_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_api_snapshot_output_path(session_id));
    write_snapshot_json(&output_path, &saved.snapshot_json)?;
    println!(
        "Saved remote session #{} from {} to {}",
        session_id.0,
        normalize_api_base(api_base),
        output_path.display()
    );
    Ok(())
}

fn cmd_load_api(
    api_base: &str,
    input_path: &Path,
    session_path: Option<&Path>,
) -> Result<(), DynError> {
    if let Some(session_path) = session_path {
        return Err(format!(
            "api mode does not use session files; remove --session {} and rerun",
            session_path.display()
        )
        .into());
    }

    let client = api_client();
    let snapshot_json = fs::read_to_string(input_path)?;
    let summary: ApiSessionSummary = api_post_json(
        &client,
        &format!("{}/sessions/load", normalize_api_base(api_base)),
        &serde_json::json!({ "snapshot_json": snapshot_json }),
    )?;
    println!(
        "Loaded remote session #{} via {} at tick {}. {}",
        summary.session_id.0,
        normalize_api_base(api_base),
        summary.current_tick.0,
        render_victory(&summary.victory)
    );
    Ok(())
}

fn cmd_run_api(api_base: &str, session_arg: &Path) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary: ApiSessionSummary = api_post_empty(
        &client,
        &format!(
            "{}/sessions/{}/run",
            normalize_api_base(api_base),
            session_id.0
        ),
    )?;
    println!(
        "Session #{} is {} at tick {}. {}",
        session_id.0,
        render_control_state(summary.control_state),
        summary.current_tick.0,
        render_victory(&summary.victory)
    );
    Ok(())
}

fn cmd_scenario_run_api(
    api_base: &str,
    _ruleset_path: Option<&Path>,
    _scenario_path: Option<&Path>,
    _session_path: Option<PathBuf>,
    _ticks: u32,
) -> Result<(), DynError> {
    Err(format!(
        "scenario-run is currently file-backed only; the server at {} chooses its scenario at startup",
        normalize_api_base(api_base)
    )
    .into())
}

fn cmd_pause_api(api_base: &str, session_arg: &Path) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary: ApiSessionSummary = api_post_empty(
        &client,
        &format!(
            "{}/sessions/{}/pause",
            normalize_api_base(api_base),
            session_id.0
        ),
    )?;
    println!(
        "Session #{} is {} at tick {}. {}",
        session_id.0,
        render_control_state(summary.control_state),
        summary.current_tick.0,
        render_victory(&summary.victory)
    );
    Ok(())
}

fn cmd_step_api(api_base: &str, session_arg: &Path, ticks: u32) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary: ApiSessionSummary = api_post_json(
        &client,
        &format!(
            "{}/sessions/{}/step",
            normalize_api_base(api_base),
            session_id.0
        ),
        &StepSessionRequest { ticks },
    )?;
    println!(
        "Advanced to tick {}. {}",
        summary.current_tick.0,
        render_victory(&summary.victory)
    );
    Ok(())
}

fn cmd_mutate_api(
    api_base: &str,
    session_arg: &Path,
    player_id: PlayerId,
    command: CommandKind,
    success_message: &str,
) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary: ApiSessionSummary = api_post_json(
        &client,
        &format!(
            "{}/sessions/{}/commands",
            normalize_api_base(api_base),
            session_id.0
        ),
        &IssueCommandRequest {
            player_id: player_id.0,
            command,
        },
    )?;
    println!("{success_message} at tick {}", summary.current_tick.0);
    Ok(())
}

fn api_client() -> Client {
    Client::new()
}

fn normalize_api_base(api_base: &str) -> &str {
    api_base.trim_end_matches('/')
}

fn parse_session_id_arg(session_arg: &Path) -> Result<SessionId, DynError> {
    let raw = session_arg.to_string_lossy();
    let value = raw.parse::<u64>().map_err(|_| {
        format!(
            "api mode expects --session to be a numeric session id, got '{}'",
            raw
        )
    })?;
    Ok(SessionId::new(value))
}

fn api_session_summary(
    client: &Client,
    api_base: &str,
    session_id: SessionId,
) -> Result<ApiSessionSummary, DynError> {
    api_get_json(
        client,
        &format!("{}/sessions/{}", normalize_api_base(api_base), session_id.0),
    )
}

fn api_player_view(
    client: &Client,
    api_base: &str,
    session_id: SessionId,
    player_id: PlayerId,
) -> Result<PlayerStateView, DynError> {
    api_get_json(
        client,
        &format!(
            "{}/sessions/{}/state?player_id={}",
            normalize_api_base(api_base),
            session_id.0,
            player_id.0
        ),
    )
}

fn api_player_events(
    client: &Client,
    api_base: &str,
    session_id: SessionId,
    player_id: PlayerId,
    from_tick: TickId,
) -> Result<Vec<EventRecord>, DynError> {
    api_get_json(
        client,
        &format!(
            "{}/sessions/{}/events?player_id={}&from_tick={}",
            normalize_api_base(api_base),
            session_id.0,
            player_id.0,
            from_tick.0
        ),
    )
}

fn api_get_json<T: DeserializeOwned>(client: &Client, url: &str) -> Result<T, DynError> {
    let response = client.get(url).send()?;
    api_response_json(response)
}

fn api_post_empty<T: DeserializeOwned>(client: &Client, url: &str) -> Result<T, DynError> {
    let response = client.post(url).send()?;
    api_response_json(response)
}

fn api_post_json<T: DeserializeOwned, B: Serialize>(
    client: &Client,
    url: &str,
    body: &B,
) -> Result<T, DynError> {
    let response = client.post(url).json(body).send()?;
    api_response_json(response)
}

fn api_response_json<T: DeserializeOwned>(
    response: reqwest::blocking::Response,
) -> Result<T, DynError> {
    let status = response.status();
    if status.is_success() {
        Ok(response.json()?)
    } else {
        let body = response.text()?;
        Err(format!("api request failed ({status}): {body}").into())
    }
}

fn print_status(
    session_label: &str,
    control_state: Option<SessionControlState>,
    victory: &VictoryState,
    player_id: PlayerId,
    view: &PlayerStateView,
) {
    let owned_count = view
        .locations
        .iter()
        .filter(|location| location.visibility == starforge_core::LocationVisibility::Owned)
        .count();

    println!("Session: {session_label}");
    println!("Tick: {}", view.tick_id.0);
    if let Some(control_state) = control_state {
        println!("Control: {}", render_control_state(control_state));
    }
    println!("Victory: {}", render_victory(victory));
    println!("Player: P{}", player_id.0);
    println!("Model tier: {}", view.model_tier);
    println!("Collapse: {}", render_collapse(&view.collapse));
    println!("Owned worlds: {}", owned_count);
    println!(
        "Throughput: available={} upkeep={} research={} training={} agents={}",
        view.throughput.available,
        view.throughput.reserved_for_model_upkeep,
        view.throughput.reserved_for_research,
        view.throughput.reserved_for_training,
        view.throughput.reserved_for_agents
    );
    println!(
        "Research: industry={} models={} warfare={} resilience={}",
        view.research.industry_level,
        view.research.models_level,
        view.research.warfare_level,
        view.research.resilience_level
    );
    match &view.research.active_project {
        Some(project) => println!(
            "Research project: {} level {} progress {}/{} requiring {} research throughput",
            render_research_branch(project.branch),
            project.target_level,
            project.progress_ticks,
            project.required_ticks,
            project.required_research_throughput
        ),
        None => println!("Research project: none"),
    }
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
        Some(training) => {
            let site_suffix = training
                .ascension_site_location_id
                .map(|location_id| format!(" site=#{location_id}"))
                .unwrap_or_default();
            println!(
                "Training: tier {} progress {}/{} requiring {} training throughput{}",
                training.target_tier,
                training.progress_ticks,
                training.required_ticks,
                training.required_training_throughput,
                site_suffix
            );
        }
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
}

fn print_metrics(session_label: &str, metrics: &SessionMetrics) {
    println!("Session: {session_label}");
    println!("Tick: {}", metrics.current_tick.0);
    println!("Control: {}", render_control_state(metrics.control_state));
    println!("Events: {}", metrics.event_count);
    println!("Accepted commands: {}", metrics.accepted_command_count);
    println!("Pending commands: {}", metrics.pending_command_count);
    println!("In-flight transits: {}", metrics.transit_count);
}

fn load_session(session_path: &Path) -> Result<GameSession, DynError> {
    let json = fs::read_to_string(session_path)?;
    Ok(GameSession::from_snapshot_json(&json)?)
}

fn write_snapshot_json(output_path: &Path, snapshot_json: &str) -> Result<(), DynError> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output_path, snapshot_json)?;
    Ok(())
}

fn save_session(session_path: &Path, session: &GameSession) -> Result<(), DynError> {
    write_snapshot_json(session_path, &session.snapshot_json()?)
}

fn default_session_path() -> PathBuf {
    PathBuf::from("starforge-session.json")
}

fn default_snapshot_output_path(session_path: &Path) -> PathBuf {
    let parent = session_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let stem = session_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("starforge-session");
    let file_name = format!("{stem}-snapshot.json");
    parent.join(file_name)
}

fn default_api_snapshot_output_path(session_id: SessionId) -> PathBuf {
    PathBuf::from(format!("session-{}-snapshot.json", session_id.0))
}

fn load_cli_harness(
    ruleset_path: Option<&Path>,
    scenario_path: Option<&Path>,
) -> Result<ScenarioHarness, DynError> {
    let ruleset_path = ruleset_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ruleset_path);
    let scenario_path = scenario_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_scenario_path);
    Ok(load_harness(&ruleset_path, &scenario_path)?)
}

fn print_created_session(
    session_path: &Path,
    harness: &ScenarioHarness,
    session: &GameSession,
    headline: &str,
) {
    println!("{headline} {}", session_path.display());
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
        first_recommended_route(session, PlayerId::new(1))
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

fn render_known_routes(routes: &[LocationConnection]) -> Vec<String> {
    routes
        .iter()
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
    if let Some(contesting_players) = &location.contesting_players
        && !contesting_players.is_empty()
    {
        summary.push_str(&format!(" contesting={contesting_players:?}"));
    }
    if let Some(pacification_ticks_remaining) = location.pacification_ticks_remaining
        && pacification_ticks_remaining > 0
    {
        summary.push_str(&format!(" pacification={pacification_ticks_remaining}"));
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

fn render_control_state(control_state: SessionControlState) -> &'static str {
    match control_state {
        SessionControlState::Running => "running",
        SessionControlState::Paused => "paused",
    }
}

fn render_collapse(collapse: &CommandCollapseState) -> String {
    match collapse {
        CommandCollapseState::Stable => "stable".to_owned(),
        CommandCollapseState::Collapsing { ticks_remaining } => {
            format!("collapsing ({ticks_remaining} ticks remaining)")
        }
        CommandCollapseState::Defeated => "defeated".to_owned(),
    }
}

fn render_transit_kind(kind: &TransitKind) -> &'static str {
    match kind {
        TransitKind::Survey => "survey",
        TransitKind::Pacification => "pacify",
        TransitKind::Claim => "claim",
        TransitKind::Assault => "assault",
        TransitKind::StrategicStrike => "strategic-strike",
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
            reserved_for_research,
            reserved_for_training,
            reserved_for_agents,
            available,
        } => format!(
            "throughput budget upkeep={} research={} training={} agents={} available={}",
            reserved_for_model_upkeep,
            reserved_for_research,
            reserved_for_training,
            reserved_for_agents,
            available
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
        starforge_core::EventKind::LocationContested {
            location_id,
            attacker_id,
            defender_id,
        } => match defender_id {
            Some(defender_id) => format!(
                "location #{} contested by P{} against P{}",
                location_id, attacker_id.0, defender_id.0
            ),
            None => format!("location #{} contested by P{}", location_id, attacker_id.0),
        },
        starforge_core::EventKind::AssaultRepelled {
            location_id,
            attacker_id,
            defender_id,
        } => format!(
            "assault on #{} by P{} repelled by P{}",
            location_id, attacker_id.0, defender_id.0
        ),
        starforge_core::EventKind::LocationCaptured {
            location_id,
            attacker_id,
            defender_id,
            pacification_ticks,
        } => format!(
            "location #{} captured by P{} from P{}; pacification={}",
            location_id, attacker_id.0, defender_id.0, pacification_ticks
        ),
        starforge_core::EventKind::PacificationCompleted {
            location_id,
            player_id,
        } => format!(
            "pacification completed at #{} for P{}",
            location_id, player_id.0
        ),
        starforge_core::EventKind::StrategicStrikeIntercepted {
            location_id,
            attacker_id,
            defender_id,
        } => format!(
            "strategic strike on #{} by P{} intercepted by P{}",
            location_id, attacker_id.0, defender_id.0
        ),
        starforge_core::EventKind::LocationDestroyed {
            location_id,
            attacker_id,
            defender_id,
        } => format!(
            "location #{} destroyed by P{} against P{}",
            location_id, attacker_id.0, defender_id.0
        ),
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
        starforge_core::EventKind::ResearchProjectStarted {
            branch,
            target_level,
            required_research_throughput,
            required_ticks,
        } => format!(
            "research started for {} level {} requiring {} throughput for {} ticks",
            render_research_branch(*branch),
            target_level,
            required_research_throughput,
            required_ticks
        ),
        starforge_core::EventKind::ResearchProjectCompleted {
            branch,
            achieved_level,
        } => format!(
            "research completed for {} level {}",
            render_research_branch(*branch),
            achieved_level
        ),
        starforge_core::EventKind::AscensionStarted {
            player_id,
            location_id,
            required_training_throughput,
            required_ticks,
        } => format!(
            "ascension started for P{} at #{} requiring {} throughput for {} ticks",
            player_id.0, location_id, required_training_throughput, required_ticks
        ),
        starforge_core::EventKind::AscensionInterrupted {
            player_id,
            location_id,
            reason,
        } => format!(
            "ascension interrupted for P{} at #{} ({})",
            player_id.0, location_id, reason
        ),
        starforge_core::EventKind::CommandCollapseStarted {
            player_id,
            ticks_remaining,
        } => format!(
            "command collapse started for P{} with {} ticks remaining",
            player_id.0, ticks_remaining
        ),
        starforge_core::EventKind::CommandCollapseRecovered { player_id } => {
            format!("command collapse recovered for P{}", player_id.0)
        }
        starforge_core::EventKind::PlayerDefeated { player_id, reason } => {
            format!("player P{} defeated ({})", player_id.0, reason)
        }
        starforge_core::EventKind::VictoryDeclared { winner, reason } => {
            format!("victory declared for P{} ({})", winner.0, reason)
        }
    }
}

fn render_research_branch(branch: ResearchBranch) -> &'static str {
    match branch {
        ResearchBranch::Industry => "industry",
        ResearchBranch::Models => "models",
        ResearchBranch::Warfare => "warfare",
        ResearchBranch::Resilience => "resilience",
    }
}
