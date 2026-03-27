use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;
use reqwest::blocking::Client;
use serde::{Serialize, de::DeserializeOwned};
use starforge_api::{
    ApiServerConfig, ApiSessionSummary, IssueCommandRequest, KnownRouteView, SaveSessionResponse,
    SessionControlState, SessionMetrics, StepSessionRequest,
};
use starforge_core::{
    CommandKind, EventRecord, GameSession, PlayerId, PlayerStateView, SessionId, TickId,
};
use starforge_scenarios::{
    ScenarioHarness, default_ruleset_path, default_scenario_path, load_harness,
    starter_skirmish_harness,
};

use crate::{
    cli::{
        Cli, Command as CliCommand, DaemonArgs, NewArgs, PlayArgs, PlayCommand,
        PlayerScopedCommand, StepArgs,
    },
    render::{
        render_event, render_known_routes, render_map_lines, render_status_lines, render_victory,
    },
};

mod cli;
mod live;
mod render;

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
    match command {
        CliCommand::Daemon(args) => tokio::runtime::Runtime::new()?.block_on(cmd_daemon(args)),
        CliCommand::Play(args) => tokio::runtime::Runtime::new()?.block_on(cmd_play(args)),
        command => match api_base.as_deref() {
            Some(api_base) => run_api(api_base, command),
            None => run_file(command),
        },
    }
}

async fn cmd_daemon(args: DaemonArgs) -> Result<(), DynError> {
    let config = ApiServerConfig {
        bind_address: args.bind_address.clone(),
        ..ApiServerConfig::default()
    };
    println!("Starforge API listening on http://{}", config.bind_address);
    starforge_api::run_server(config).await?;
    Ok(())
}

async fn cmd_play(args: PlayArgs) -> Result<(), DynError> {
    match args.command {
        PlayCommand::Create(args) => {
            live::create_and_play(args.api_url, args.player, args.mode).await
        }
        PlayCommand::Join(args) => {
            live::join_and_play(args.api_url, args.session, args.player).await
        }
    }
}

fn run_file(command: CliCommand) -> Result<(), DynError> {
    match command {
        CliCommand::New(args) => {
            println!("{}", cmd_new(args)?);
            Ok(())
        }
        CliCommand::Status(args) => run_snapshot_player_command_and_print(
            &args.session.session,
            args.player,
            PlayerScopedCommand::Status,
        ),
        CliCommand::Map(args) => run_snapshot_player_command_and_print(
            &args.session.session,
            args.player,
            PlayerScopedCommand::Map,
        ),
        CliCommand::Events(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Events(args.options),
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
        CliCommand::Step(args) => {
            println!("{}", cmd_step(args)?);
            Ok(())
        }
        CliCommand::Survey(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Survey(args.transit),
        ),
        CliCommand::Pacify(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Pacify(args.transit),
        ),
        CliCommand::Claim(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Claim(args.transit),
        ),
        CliCommand::Assault(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Assault(args.transit),
        ),
        CliCommand::Strike(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Strike(args.transit),
        ),
        CliCommand::Build(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Build(args.infrastructure),
        ),
        CliCommand::Repair(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Repair(args.infrastructure),
        ),
        CliCommand::Relay(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Relay(args.relay),
        ),
        CliCommand::Budget(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Budget(args.budget),
        ),
        CliCommand::Research(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Research(args.research),
        ),
        CliCommand::Train(args) => run_snapshot_player_command_and_print(
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Train(args.train),
        ),
        CliCommand::Daemon(_) | CliCommand::Play(_) => unreachable!("handled before file mode"),
    }
}

fn run_api(api_base: &str, command: CliCommand) -> Result<(), DynError> {
    match command {
        CliCommand::New(args) => cmd_new_api(api_base, args.session),
        CliCommand::Status(args) => run_api_player_command(
            api_base,
            &args.session.session,
            args.player,
            PlayerScopedCommand::Status,
        ),
        CliCommand::Map(args) => run_api_player_command(
            api_base,
            &args.session.session,
            args.player,
            PlayerScopedCommand::Map,
        ),
        CliCommand::Events(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Events(args.options),
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
        CliCommand::Survey(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Survey(args.transit),
        ),
        CliCommand::Pacify(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Pacify(args.transit),
        ),
        CliCommand::Claim(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Claim(args.transit),
        ),
        CliCommand::Assault(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Assault(args.transit),
        ),
        CliCommand::Strike(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Strike(args.transit),
        ),
        CliCommand::Build(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Build(args.infrastructure),
        ),
        CliCommand::Repair(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Repair(args.infrastructure),
        ),
        CliCommand::Relay(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Relay(args.relay),
        ),
        CliCommand::Budget(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Budget(args.budget),
        ),
        CliCommand::Research(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Research(args.research),
        ),
        CliCommand::Train(args) => run_api_player_command(
            api_base,
            &args.common.session.session,
            args.common.player,
            PlayerScopedCommand::Train(args.train),
        ),
        CliCommand::Daemon(_) | CliCommand::Play(_) => unreachable!("handled before api mode"),
    }
}

fn run_snapshot_player_command_and_print(
    session_path: &Path,
    player_id: PlayerId,
    command: PlayerScopedCommand,
) -> Result<(), DynError> {
    println!(
        "{}",
        run_snapshot_player_command(session_path, player_id, command)?
    );
    Ok(())
}

fn run_snapshot_player_command(
    session_path: &Path,
    player_id: PlayerId,
    command: PlayerScopedCommand,
) -> Result<String, DynError> {
    match command {
        PlayerScopedCommand::Status => {
            let session = load_session(session_path)?;
            let view = session.player_view(player_id)?;
            Ok(render_status_lines(
                &format!("Session: {}", session_path.display()),
                player_id,
                &view,
                &session.state().victory,
            )
            .join("\n"))
        }
        PlayerScopedCommand::Map => {
            let session = load_session(session_path)?;
            let view = session.player_view(player_id)?;
            let known_routes = render_known_routes(&session.state().connections, &view.locations);
            Ok(render_map_lines(
                player_id,
                view.tick_id.0,
                &session.state().victory,
                &view.locations,
                &known_routes,
            )
            .join("\n"))
        }
        PlayerScopedCommand::Events(args) => {
            let session = load_session(session_path)?;
            let events = session.player_events(player_id, TickId::new(args.from_tick))?;
            if events.is_empty() {
                Ok(format!(
                    "No visible events for P{} from tick {}",
                    player_id.0, args.from_tick
                ))
            } else {
                Ok(events
                    .iter()
                    .map(|event| format!("[{}] {}", event.tick_id.0, render_event(&event.kind)))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
        }
        command => {
            let mut session = load_session(session_path)?;
            let command_kind = command
                .to_command_kind()
                .expect("mutating snapshot commands should map to command kinds");
            let success_message = command
                .success_message()
                .expect("mutating snapshot commands should expose success messages");
            session.issue_command_now(player_id, command_kind)?;
            save_session(session_path, &session)?;
            Ok(format!(
                "{success_message} at tick {}",
                session.current_tick().0
            ))
        }
    }
}

fn run_api_player_command(
    api_base: &str,
    session_arg: &Path,
    player_id: PlayerId,
    command: PlayerScopedCommand,
) -> Result<(), DynError> {
    match command {
        PlayerScopedCommand::Status => cmd_status_api(api_base, session_arg, player_id),
        PlayerScopedCommand::Map => cmd_map_api(api_base, session_arg, player_id),
        PlayerScopedCommand::Events(args) => cmd_events_api(
            api_base,
            session_arg,
            player_id,
            TickId::new(args.from_tick),
        ),
        command => {
            let command_kind = command
                .to_command_kind()
                .expect("mutating api commands should map to command kinds");
            let success_message = command
                .success_message()
                .expect("mutating api commands should expose success messages");
            cmd_mutate_api(
                api_base,
                session_arg,
                player_id,
                command_kind,
                success_message,
            )
        }
    }
}

fn cmd_new(args: NewArgs) -> Result<String, DynError> {
    let session_path = args.session.unwrap_or_else(default_session_path);
    if session_path.exists() {
        return Err(format!("session file '{}' already exists", session_path.display()).into());
    }

    let harness = starter_skirmish_harness()?;
    let session = harness.instantiate_session(SessionId::new(1));
    save_session(&session_path, &session)?;
    Ok(created_session_output(
        "Created session at",
        &session_path,
        &harness.name,
        &session,
    ))
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

    let mut lines = created_session_lines(
        "Created scenario session at",
        &session_path,
        &harness.name,
        &session,
    );
    if ticks > 0 {
        lines.push(format!(
            "Advanced scenario session to tick {} during setup.",
            session.current_tick().0
        ));
    }
    println!("{}", lines.join("\n"));
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

fn cmd_step(args: StepArgs) -> Result<String, DynError> {
    let mut session = load_session(&args.session.session)?;
    session.advance_ticks(args.ticks);
    save_session(&args.session.session, &session)?;
    Ok(format!(
        "Advanced to tick {}. {}",
        session.current_tick().0,
        render_victory(&session.state().victory)
    ))
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
    let base = normalize_api_base(api_base);
    let summary: ApiSessionSummary = api_post_empty(&client, &format!("{base}/sessions"))?;

    println!(
        "Created remote session #{} via {base}",
        summary.session_id.0
    );
    println!("Scenario: {}", summary.scenario_name);
    println!();
    println!("Suggested first steps:");
    println!(
        "  starforge-cli --api-base {} map --session {} --player 1",
        base, summary.session_id.0
    );
    println!(
        "  starforge-cli --api-base {} status --session {} --player 1",
        base, summary.session_id.0
    );
    Ok(())
}

fn cmd_status_api(api_base: &str, session_arg: &Path, player_id: PlayerId) -> Result<(), DynError> {
    let client = api_client();
    let session_id = parse_session_id_arg(session_arg)?;
    let summary = api_session_summary(&client, api_base, session_id)?;
    let view = api_player_view(&client, api_base, session_id, player_id)?;
    print_status(
        &format!(
            "Session: #{} @ {}",
            session_id.0,
            normalize_api_base(api_base)
        ),
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
    let known_routes = view
        .routes
        .iter()
        .map(|route| KnownRouteView {
            from_location_id: route.from_location_id,
            to_location_id: route.to_location_id,
            travel_time_ticks: route.travel_time_ticks,
        })
        .collect::<Vec<_>>();

    println!(
        "{}",
        render_map_lines(
            player_id,
            view.tick_id.0,
            &summary.victory,
            &view.locations,
            &known_routes,
        )
        .join("\n")
    );
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
    victory: &starforge_core::VictoryState,
    player_id: PlayerId,
    view: &PlayerStateView,
) {
    let mut lines = render_status_lines(session_label, player_id, view, victory);
    if let Some(control_state) = control_state {
        lines.insert(
            2,
            format!("Control: {}", render_control_state(control_state)),
        );
    }
    println!("{}", lines.join("\n"));
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

fn render_control_state(control_state: SessionControlState) -> &'static str {
    match control_state {
        SessionControlState::Running => "running",
        SessionControlState::Paused => "paused",
    }
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
    parent.join(format!("{stem}-snapshot.json"))
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

fn created_session_output(
    headline: &str,
    session_path: &Path,
    scenario_name: &str,
    session: &GameSession,
) -> String {
    created_session_lines(headline, session_path, scenario_name, session).join("\n")
}

fn created_session_lines(
    headline: &str,
    session_path: &Path,
    scenario_name: &str,
    session: &GameSession,
) -> Vec<String> {
    let mut lines = vec![
        format!("{headline} {}", session_path.display()),
        format!("Scenario: {}", scenario_name),
        "Players:".to_owned(),
    ];
    for location in session
        .state()
        .locations
        .iter()
        .filter(|location| location.homeworld_of.is_some())
    {
        let player_id = location
            .homeworld_of
            .expect("homeworld should map to a player");
        lines.push(format!(
            "  P{} homeworld: {} (# {})",
            player_id.0, location.name, location.location_id
        ));
    }
    lines.push(String::new());
    lines.push("Suggested first steps:".to_owned());
    lines.push(format!(
        "  starforge-cli map --session {} --player 1",
        session_path.display()
    ));
    lines.push(format!(
        "  starforge-cli status --session {} --player 1",
        session_path.display()
    ));
    if let Some((origin_id, destination_id, travel_time)) =
        first_recommended_route(session, PlayerId::new(1))
    {
        lines.push(format!(
            "  starforge-cli survey --session {} --player 1 --origin {} --destination {}",
            session_path.display(),
            origin_id,
            destination_id
        ));
        lines.push(format!(
            "  starforge-cli step --session {} --ticks {}",
            session_path.display(),
            travel_time
        ));
    }
    lines
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

#[cfg(test)]
fn live_test_frame() -> starforge_api::PlayerFrameResponse {
    use starforge_api::{
        CreateSessionRequest, JoinSessionRequest, LiveSessionSummary, PlayerAlert, PlayerAlertKind,
        PlayerFrameResponse, PlayerSeat, ReadySessionRequest, RunnerSpeedRequest, RunnerStatus,
        SessionInfoResponse, SessionMode, SessionPhase,
    };
    use starforge_core::{
        BuildCapacity, CommandCollapseState, EnergyPotential, EventKind, IndexedEventRecord,
        LocationEconomyState, LocationKind, LocationView, LocationVisibility, PlayerEconomyState,
        PlayerResearchState, PlayerStateView, RelayStatus, ResourceRichness, ResourceStockpiles,
        TerritoryState, ThroughputBudget, VictoryState, VisibilityState,
    };

    let _ = (
        std::any::TypeId::of::<CreateSessionRequest>(),
        std::any::TypeId::of::<JoinSessionRequest>(),
        std::any::TypeId::of::<ReadySessionRequest>(),
        std::any::TypeId::of::<RunnerSpeedRequest>(),
        std::any::TypeId::of::<SessionInfoResponse>(),
    );

    PlayerFrameResponse {
        session_id: SessionId::new(1),
        summary: LiveSessionSummary {
            scenario_name: "two_player_skirmish".to_owned(),
            current_tick: TickId::new(10),
            victory: VictoryState::Ongoing,
        },
        seats: vec![
            PlayerSeat {
                player_id: PlayerId::new(1),
                claimed: true,
                ready: false,
            },
            PlayerSeat {
                player_id: PlayerId::new(2),
                claimed: false,
                ready: false,
            },
        ],
        runner: RunnerStatus {
            mode: SessionMode::Competitive,
            phase: SessionPhase::Lobby,
            tick_interval_ms: 250,
            pause_allowed: false,
            speed_change_allowed: false,
            paused: true,
        },
        state_hash: 42,
        next_event_index: 3,
        view: PlayerStateView {
            tick_id: TickId::new(10),
            player_id: PlayerId::new(1),
            model_tier: 1,
            economy: PlayerEconomyState {
                total_connected_energy: 60,
                total_connected_datacenter_capacity: 50,
                usable_throughput: 50,
                connected_stockpiles: ResourceStockpiles {
                    common_materials: 500,
                    volatiles: 120,
                    rare_materials: 60,
                },
                disconnected_owned_location_ids: Vec::new(),
            },
            throughput: ThroughputBudget {
                reserved_for_model_upkeep: 0,
                reserved_for_research: 0,
                reserved_for_training: 0,
                reserved_for_agents: 0,
                available: 50,
            },
            research: PlayerResearchState::default(),
            training: None,
            collapse: CommandCollapseState::Stable,
            visibility: VisibilityState::default(),
            locations: vec![
                LocationView {
                    location_id: 1,
                    name: "Helios".to_owned(),
                    visibility: LocationVisibility::Owned,
                    territory: TerritoryState::Owned,
                    controller: Some(PlayerId::new(1)),
                    contesting_players: None,
                    pacification_ticks_remaining: Some(0),
                    kind: Some(LocationKind::HabitablePlanet),
                    resource_richness: Some(ResourceRichness::Rich),
                    energy_potential: Some(EnergyPotential::High),
                    build_capacity: Some(BuildCapacity::Expansive),
                    relay_status: Some(RelayStatus::Connected),
                    orbital_slots: Some(3),
                    has_environmental_hazard: Some(false),
                    infrastructure: Some(Vec::new()),
                    infrastructure_projects: Some(Vec::new()),
                    economy: Some(LocationEconomyState::default()),
                    stockpiles: Some(ResourceStockpiles::default()),
                    hostile_remnant_present: Some(false),
                },
                LocationView {
                    location_id: 2,
                    name: "Verdant 2".to_owned(),
                    visibility: LocationVisibility::Observed,
                    territory: TerritoryState::Neutral,
                    controller: None,
                    contesting_players: None,
                    pacification_ticks_remaining: Some(0),
                    kind: Some(LocationKind::HabitablePlanet),
                    resource_richness: Some(ResourceRichness::Moderate),
                    energy_potential: Some(EnergyPotential::Moderate),
                    build_capacity: Some(BuildCapacity::Standard),
                    relay_status: Some(RelayStatus::Disconnected),
                    orbital_slots: Some(2),
                    has_environmental_hazard: Some(false),
                    infrastructure: Some(Vec::new()),
                    infrastructure_projects: Some(Vec::new()),
                    economy: Some(LocationEconomyState::default()),
                    stockpiles: Some(ResourceStockpiles::default()),
                    hostile_remnant_present: Some(false),
                },
            ],
            routes: vec![starforge_core::LocationConnection {
                from_location_id: 1,
                to_location_id: 2,
                travel_time_ticks: 50,
            }],
            transits: Vec::new(),
        },
        events: vec![IndexedEventRecord {
            event_index: 2,
            record: starforge_core::EventRecord {
                tick_id: TickId::new(10),
                player_id: Some(PlayerId::new(1)),
                kind: EventKind::LocationSurveyed { location_id: 2 },
            },
        }],
        alerts: vec![PlayerAlert {
            kind: PlayerAlertKind::Survey,
            title: "location #2 surveyed".to_owned(),
            tick_id: TickId::new(10),
            location_id: Some(2),
        }],
        known_routes: vec![KnownRouteView {
            from_location_id: 1,
            to_location_id: 2,
            travel_time_ticks: 50,
        }],
    }
}
