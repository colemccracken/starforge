use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use starforge_api::SessionMode;
use starforge_core::{InfrastructureKind, PlayerId, RelayStatus, ResearchBranch, SessionId};

pub(crate) const DEFAULT_API_URL: &str = "http://127.0.0.1:8080";
pub(crate) const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";

const AFTER_HELP: &str = r#"Command Quick Reference:
  starforge-cli daemon [--bind-address <ADDR>]
  starforge-cli play create --player <PLAYER> [--mode <MODE>] [--api-url <URL>]
  starforge-cli play join --session <SESSION> --player <PLAYER> [--api-url <URL>]
  starforge-cli new [--session <PATH>]
  starforge-cli status --session <PATH> --player <PLAYER>
  starforge-cli map --session <PATH> --player <PLAYER>
  starforge-cli events --session <PATH> --player <PLAYER> [--from-tick <TICK>]
  starforge-cli metrics --session <PATH>
  starforge-cli save --session <PATH> [--output <PATH>]
  starforge-cli load --input <PATH> [--session <PATH>]
  starforge-cli scenario-run [--ruleset <PATH>] [--scenario <PATH>] [--session <PATH>] [--ticks <TICKS>]
  starforge-cli run --session <PATH>
  starforge-cli pause --session <PATH>
  starforge-cli step --session <PATH> --ticks <TICKS>
  starforge-cli survey --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli pacify --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli claim --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli assault --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli strike --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli develop --session <PATH> --player <PLAYER> --location <LOCATION> --kind <KIND>
  starforge-cli repair --session <PATH> --player <PLAYER> --location <LOCATION> --kind <KIND>
  starforge-cli relay --session <PATH> --player <PLAYER> --location <LOCATION> --status <STATUS>
  starforge-cli budget --session <PATH> --player <PLAYER> --upkeep <N> --research <N> --training <N> --agents <N>
  starforge-cli research --session <PATH> --player <PLAYER> --branch <BRANCH> --target-level <LEVEL>
  starforge-cli train --session <PATH> --player <PLAYER> --target-tier <TIER>

HTTP API mode:
  Add `--api-base http://127.0.0.1:8080` before the subcommand.
  In API mode, `--session` should be the numeric session id returned by `new`.
  Example: starforge-cli --api-base http://127.0.0.1:8080 status --session 1 --player 1

Aliases:
  --session also supports -s
  --player also supports -p
  All other command flags are long-form only.

Prototype loop:
  1. Create a session with `new`.
  2. Use `map` and `status` to inspect player-visible state.
  3. `survey` nearby neutral worlds, then `step` until arrival.
  4. If a surveyed world has `remnant=true`, use `pacify` and `step`.
  5. `claim` cleared neutral worlds, then expand with `develop` and `repair`.
  6. Use `budget` to reserve research and training throughput, then use `research` and `train`.
  7. A completed tier 5 training run ends the match immediately.
"#;

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "starforge-cli",
    about = "Starforge CLI",
    arg_required_else_help = true,
    subcommand_required = true,
    after_help = AFTER_HELP
)]
pub(crate) struct Cli {
    #[arg(long, global = true, value_name = "URL")]
    pub(crate) api_base: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum Command {
    Daemon(DaemonArgs),
    Play(PlayArgs),
    New(NewArgs),
    Status(SessionPlayerArgs),
    Map(SessionPlayerArgs),
    Events(EventsArgs),
    Metrics(SessionArg),
    Save(SaveArgs),
    Load(LoadArgs),
    ScenarioRun(ScenarioRunArgs),
    Run(SessionArg),
    Pause(SessionArg),
    Step(StepArgs),
    Survey(TransitArgs),
    Pacify(TransitArgs),
    Claim(TransitArgs),
    Assault(TransitArgs),
    Strike(TransitArgs),
    #[command(name = "develop", alias = "build")]
    Build(InfrastructureArgs),
    Repair(InfrastructureArgs),
    Relay(RelayArgs),
    Budget(BudgetArgs),
    Research(ResearchArgs),
    Train(TrainArgs),
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct DaemonArgs {
    #[arg(long, default_value = DEFAULT_BIND_ADDRESS, value_name = "ADDR")]
    pub(crate) bind_address: String,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct PlayArgs {
    #[command(subcommand)]
    pub(crate) command: PlayCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq, Clone)]
pub(crate) enum PlayCommand {
    Create(PlayCreateArgs),
    Join(PlayJoinArgs),
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct PlayCreateArgs {
    #[arg(long, short = 'p', value_name = "PLAYER", value_parser = parse_player_id)]
    pub(crate) player: PlayerId,
    #[arg(long, default_value = "competitive", value_name = "MODE", value_parser = parse_session_mode)]
    pub(crate) mode: SessionMode,
    #[arg(long, default_value = DEFAULT_API_URL, value_name = "URL")]
    pub(crate) api_url: String,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct PlayJoinArgs {
    #[arg(long, short = 's', value_name = "SESSION", value_parser = parse_session_id)]
    pub(crate) session: SessionId,
    #[arg(long, short = 'p', value_name = "PLAYER", value_parser = parse_player_id)]
    pub(crate) player: PlayerId,
    #[arg(long, default_value = DEFAULT_API_URL, value_name = "URL")]
    pub(crate) api_url: String,
}

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "player-scoped",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub(crate) struct PlayerScopedCli {
    #[command(subcommand)]
    pub(crate) command: PlayerScopedCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq, Clone)]
pub(crate) enum PlayerScopedCommand {
    Status,
    Map,
    Events(PlayerEventsArgs),
    Survey(TransitSpec),
    Pacify(TransitSpec),
    Claim(TransitSpec),
    Assault(TransitSpec),
    Strike(TransitSpec),
    #[command(name = "develop", alias = "build")]
    Build(ScopedInfrastructureArgs),
    Repair(ScopedInfrastructureArgs),
    Relay(ScopedRelayArgs),
    Budget(ScopedBudgetArgs),
    Research(ScopedResearchArgs),
    Train(ScopedTrainArgs),
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct NewArgs {
    #[arg(long, short = 's', value_name = "PATH")]
    pub(crate) session: Option<PathBuf>,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct SessionArg {
    #[arg(long, short = 's', value_name = "PATH")]
    pub(crate) session: PathBuf,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct SessionPlayerArgs {
    #[command(flatten)]
    pub(crate) session: SessionArg,
    #[arg(long, short = 'p', value_name = "PLAYER", value_parser = parse_player_id)]
    pub(crate) player: PlayerId,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct PlayerEventsArgs {
    #[arg(long, default_value_t = 0, value_name = "TICK")]
    pub(crate) from_tick: u64,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct EventsArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) options: PlayerEventsArgs,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct StepArgs {
    #[command(flatten)]
    pub(crate) session: SessionArg,
    #[arg(long, value_name = "TICKS")]
    pub(crate) ticks: u32,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct SaveArgs {
    #[command(flatten)]
    pub(crate) session: SessionArg,
    #[arg(long, value_name = "PATH")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct LoadArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) input: PathBuf,
    #[arg(long, short = 's', value_name = "PATH")]
    pub(crate) session: Option<PathBuf>,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ScenarioRunArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) ruleset: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub(crate) scenario: Option<PathBuf>,
    #[arg(long, short = 's', value_name = "PATH")]
    pub(crate) session: Option<PathBuf>,
    #[arg(long, default_value_t = 0, value_name = "TICKS")]
    pub(crate) ticks: u32,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct TransitSpec {
    #[arg(long, value_name = "LOCATION")]
    pub(crate) origin: u32,
    #[arg(long, value_name = "LOCATION")]
    pub(crate) destination: u32,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct TransitArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) transit: TransitSpec,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ScopedInfrastructureArgs {
    #[arg(long, value_name = "LOCATION")]
    pub(crate) location: u32,
    #[arg(long, value_name = "KIND", value_parser = parse_infrastructure_kind)]
    pub(crate) kind: InfrastructureKind,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct InfrastructureArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) infrastructure: ScopedInfrastructureArgs,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ScopedRelayArgs {
    #[arg(long, value_name = "LOCATION")]
    pub(crate) location: u32,
    #[arg(long, value_name = "STATUS", value_parser = parse_relay_status)]
    pub(crate) status: RelayStatus,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct RelayArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) relay: ScopedRelayArgs,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ScopedBudgetArgs {
    #[arg(long, value_name = "N")]
    pub(crate) upkeep: u32,
    #[arg(long, value_name = "N")]
    pub(crate) research: u32,
    #[arg(long, value_name = "N")]
    pub(crate) training: u32,
    #[arg(long, value_name = "N")]
    pub(crate) agents: u32,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct BudgetArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) budget: ScopedBudgetArgs,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ScopedResearchArgs {
    #[arg(long, value_name = "BRANCH", value_parser = parse_research_branch)]
    pub(crate) branch: ResearchBranch,
    #[arg(long, value_name = "LEVEL")]
    pub(crate) target_level: u8,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ResearchArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) research: ScopedResearchArgs,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct ScopedTrainArgs {
    #[arg(long, value_name = "TIER")]
    pub(crate) target_tier: u8,
}

#[derive(Debug, Args, PartialEq, Eq, Clone)]
pub(crate) struct TrainArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[command(flatten)]
    pub(crate) train: ScopedTrainArgs,
}

pub(crate) fn parse_player_scoped_command(input: &str) -> Result<PlayerScopedCommand, clap::Error> {
    let mut argv = vec!["player-scoped".to_owned()];
    argv.extend(input.split_whitespace().map(ToOwned::to_owned));
    PlayerScopedCli::try_parse_from(argv).map(|cli| cli.command)
}

impl PlayerScopedCommand {
    pub(crate) fn to_command_kind(&self) -> Option<starforge_core::CommandKind> {
        match self {
            Self::Status | Self::Map | Self::Events(_) => None,
            Self::Survey(args) => Some(starforge_core::CommandKind::DispatchSurveyTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            }),
            Self::Pacify(args) => Some(starforge_core::CommandKind::DispatchPacificationTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            }),
            Self::Claim(args) => Some(starforge_core::CommandKind::DispatchClaimTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            }),
            Self::Assault(args) => Some(starforge_core::CommandKind::DispatchAssaultTransit {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            }),
            Self::Strike(args) => Some(starforge_core::CommandKind::DispatchStrategicStrike {
                origin_location_id: args.origin,
                destination_location_id: args.destination,
            }),
            Self::Build(args) => Some(
                starforge_core::CommandKind::QueueInfrastructureDevelopment {
                    location_id: args.location,
                    infrastructure_kind: args.kind.clone(),
                },
            ),
            Self::Repair(args) => Some(starforge_core::CommandKind::QueueInfrastructureRepair {
                location_id: args.location,
                infrastructure_kind: args.kind.clone(),
            }),
            Self::Relay(args) => Some(starforge_core::CommandKind::SetRelayStatus {
                location_id: args.location,
                relay_status: args.status.clone(),
            }),
            Self::Budget(args) => Some(starforge_core::CommandKind::SetThroughputBudget {
                reserved_for_model_upkeep: args.upkeep,
                reserved_for_research: args.research,
                reserved_for_training: args.training,
                reserved_for_agents: args.agents,
            }),
            Self::Research(args) => Some(starforge_core::CommandKind::StartResearchProject {
                branch: args.branch,
                target_level: args.target_level,
            }),
            Self::Train(args) => Some(starforge_core::CommandKind::StartTrainingRun {
                target_tier: args.target_tier,
            }),
        }
    }

    pub(crate) fn success_message(&self) -> Option<&'static str> {
        match self {
            Self::Status | Self::Map | Self::Events(_) => None,
            Self::Survey(_) => Some("survey expedition queued"),
            Self::Pacify(_) => Some("pacification expedition queued"),
            Self::Claim(_) => Some("claim expedition queued"),
            Self::Assault(_) => Some("assault expedition queued"),
            Self::Strike(_) => Some("strategic strike queued"),
            Self::Build(_) => Some("development queued"),
            Self::Repair(_) => Some("repair queued"),
            Self::Relay(_) => Some("relay status updated"),
            Self::Budget(_) => Some("throughput budget updated"),
            Self::Research(_) => Some("research project started"),
            Self::Train(_) => Some("training run started"),
        }
    }
}

fn parse_player_id(value: &str) -> Result<PlayerId, String> {
    value
        .parse::<u8>()
        .map(PlayerId::new)
        .map_err(|_| format!("invalid player: '{value}'"))
}

fn parse_session_id(value: &str) -> Result<SessionId, String> {
    value
        .parse::<u64>()
        .map(SessionId::new)
        .map_err(|_| format!("invalid session id: '{value}'"))
}

fn parse_session_mode(value: &str) -> Result<SessionMode, String> {
    match normalize_token(value).as_str() {
        "competitive" => Ok(SessionMode::Competitive),
        "sandbox" => Ok(SessionMode::Sandbox),
        _ => Err(format!("unknown session mode '{value}'")),
    }
}

fn parse_infrastructure_kind(value: &str) -> Result<InfrastructureKind, String> {
    match normalize_token(value).as_str() {
        "commandnexus" => Ok(InfrastructureKind::CommandNexus),
        "miningsite" => Ok(InfrastructureKind::MiningSite),
        "energyproducer" => Ok(InfrastructureKind::EnergyProducer),
        "datacenter" => Ok(InfrastructureKind::Datacenter),
        "relayuplink" => Ok(InfrastructureKind::RelayUplink),
        "shipyardring" => Ok(InfrastructureKind::ShipyardRing),
        "militaryworks" => Ok(InfrastructureKind::MilitaryWorks),
        "grounddefensesite" => Ok(InfrastructureKind::GroundDefenseSite),
        _ => Err(format!("unknown infrastructure kind '{value}'")),
    }
}

fn parse_relay_status(value: &str) -> Result<RelayStatus, String> {
    match normalize_token(value).as_str() {
        "connected" => Ok(RelayStatus::Connected),
        "disconnected" => Ok(RelayStatus::Disconnected),
        _ => Err(format!("unknown relay status '{value}'")),
    }
}

fn parse_research_branch(value: &str) -> Result<ResearchBranch, String> {
    match normalize_token(value).as_str() {
        "industry" => Ok(ResearchBranch::Industry),
        "models" | "model" => Ok(ResearchBranch::Models),
        "warfare" => Ok(ResearchBranch::Warfare),
        "resilience" => Ok(ResearchBranch::Resilience),
        _ => Err(format!("unknown research branch '{value}'")),
    }
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;
    use starforge_api::SessionMode;
    use starforge_core::{InfrastructureKind, PlayerId, RelayStatus, ResearchBranch, SessionId};

    use super::{
        BudgetArgs, Cli, Command, DaemonArgs, NewArgs, PlayArgs, PlayCommand, PlayCreateArgs,
        PlayJoinArgs, PlayerEventsArgs, PlayerScopedCommand, ResearchArgs, SaveArgs,
        ScenarioRunArgs, ScopedBudgetArgs, ScopedResearchArgs, SessionArg, SessionPlayerArgs,
        TransitArgs, TransitSpec, parse_infrastructure_kind, parse_player_scoped_command,
        parse_relay_status, parse_research_branch,
    };

    fn parse_ok(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("arguments should parse successfully")
    }

    #[test]
    fn daemon_parses_bind_address() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "daemon",
                "--bind-address",
                "127.0.0.1:9000"
            ]),
            Cli {
                api_base: None,
                command: Command::Daemon(DaemonArgs {
                    bind_address: "127.0.0.1:9000".to_owned(),
                }),
            }
        );
    }

    #[test]
    fn play_create_parses_player_mode_and_api_url() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "play",
                "create",
                "--player",
                "1",
                "--mode",
                "sandbox",
                "--api-url",
                "http://127.0.0.1:9999",
            ]),
            Cli {
                api_base: None,
                command: Command::Play(PlayArgs {
                    command: PlayCommand::Create(PlayCreateArgs {
                        player: PlayerId::new(1),
                        mode: SessionMode::Sandbox,
                        api_url: "http://127.0.0.1:9999".to_owned(),
                    }),
                }),
            }
        );
    }

    #[test]
    fn play_join_parses_session_and_player() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "play",
                "join",
                "--session",
                "7",
                "--player",
                "2",
            ]),
            Cli {
                api_base: None,
                command: Command::Play(PlayArgs {
                    command: PlayCommand::Join(PlayJoinArgs {
                        session: SessionId::new(7),
                        player: PlayerId::new(2),
                        api_url: super::DEFAULT_API_URL.to_owned(),
                    }),
                }),
            }
        );
    }

    #[test]
    fn status_accepts_global_api_base_flag() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "--api-base",
                "http://127.0.0.1:8080",
                "status",
                "-s",
                "1",
                "-p",
                "1",
            ]),
            Cli {
                api_base: Some("http://127.0.0.1:8080".to_owned()),
                command: Command::Status(SessionPlayerArgs {
                    session: SessionArg {
                        session: PathBuf::from("1"),
                    },
                    player: PlayerId::new(1),
                }),
            }
        );
    }

    #[test]
    fn save_and_load_parse_named_arguments() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "save",
                "-s",
                "session.json",
                "--output",
                "out.json"
            ]),
            Cli {
                api_base: None,
                command: Command::Save(SaveArgs {
                    session: SessionArg {
                        session: PathBuf::from("session.json"),
                    },
                    output: Some(PathBuf::from("out.json")),
                }),
            }
        );
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "load",
                "--input",
                "snapshot.json",
                "-s",
                "session.json",
            ]),
            Cli {
                api_base: None,
                command: Command::Load(super::LoadArgs {
                    input: PathBuf::from("snapshot.json"),
                    session: Some(PathBuf::from("session.json")),
                }),
            }
        );
    }

    #[test]
    fn scenario_run_and_budget_parse_named_arguments() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "scenario-run",
                "--ruleset",
                "ruleset.yaml",
                "--scenario",
                "scenario.yaml",
                "-s",
                "session.json",
                "--ticks",
                "10",
            ]),
            Cli {
                api_base: None,
                command: Command::ScenarioRun(ScenarioRunArgs {
                    ruleset: Some(PathBuf::from("ruleset.yaml")),
                    scenario: Some(PathBuf::from("scenario.yaml")),
                    session: Some(PathBuf::from("session.json")),
                    ticks: 10,
                }),
            }
        );
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "budget",
                "-s",
                "session.json",
                "-p",
                "1",
                "--upkeep",
                "0",
                "--research",
                "24",
                "--training",
                "20",
                "--agents",
                "0",
            ]),
            Cli {
                api_base: None,
                command: Command::Budget(BudgetArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    budget: ScopedBudgetArgs {
                        upkeep: 0,
                        research: 24,
                        training: 20,
                        agents: 0,
                    },
                }),
            }
        );
    }

    #[test]
    fn research_command_parses_branch_and_level() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "research",
                "-s",
                "session.json",
                "-p",
                "1",
                "--branch",
                "models",
                "--target-level",
                "2",
            ]),
            Cli {
                api_base: None,
                command: Command::Research(ResearchArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    research: ScopedResearchArgs {
                        branch: ResearchBranch::Models,
                        target_level: 2,
                    },
                }),
            }
        );
    }

    #[test]
    fn player_scoped_parser_supports_research_and_events() {
        assert_eq!(
            parse_player_scoped_command("research --branch industry --target-level 1")
                .expect("player scoped command should parse"),
            PlayerScopedCommand::Research(ScopedResearchArgs {
                branch: ResearchBranch::Industry,
                target_level: 1,
            })
        );
        assert_eq!(
            parse_player_scoped_command("events --from-tick 12")
                .expect("player scoped command should parse"),
            PlayerScopedCommand::Events(PlayerEventsArgs { from_tick: 12 })
        );
    }

    #[test]
    fn strike_accepts_transit_arguments() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "strike",
                "-s",
                "session.json",
                "-p",
                "1",
                "--origin",
                "3",
                "--destination",
                "9",
            ]),
            Cli {
                api_base: None,
                command: Command::Strike(TransitArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    transit: TransitSpec {
                        origin: 3,
                        destination: 9,
                    },
                }),
            }
        );
    }

    #[test]
    fn new_defaults_session_path_when_flag_is_omitted() {
        assert_eq!(
            parse_ok(&["starforge-cli", "new"]),
            Cli {
                api_base: None,
                command: Command::New(NewArgs { session: None }),
            }
        );
    }

    #[test]
    fn infrastructure_relay_and_research_parsers_accept_aliases() {
        assert_eq!(
            parse_infrastructure_kind("ground defense site").expect("kind should parse"),
            InfrastructureKind::GroundDefenseSite
        );
        assert_eq!(
            parse_relay_status("disconnected").expect("status should parse"),
            RelayStatus::Disconnected
        );
        assert_eq!(
            parse_research_branch("model").expect("branch should parse"),
            ResearchBranch::Models
        );
    }
}
