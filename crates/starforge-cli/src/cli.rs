use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use starforge_core::{InfrastructureKind, PlayerId, RelayStatus};

const AFTER_HELP: &str = r#"Command Quick Reference:
  starforge-cli new [--session <PATH>]
  starforge-cli status --session <PATH> --player <PLAYER>
  starforge-cli map --session <PATH> --player <PLAYER>
  starforge-cli events --session <PATH> --player <PLAYER> [--from-tick <TICK>]
  starforge-cli step --session <PATH> --ticks <TICKS>
  starforge-cli survey --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli pacify --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli claim --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli assault --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli strike --session <PATH> --player <PLAYER> --origin <LOCATION> --destination <LOCATION>
  starforge-cli build --session <PATH> --player <PLAYER> --location <LOCATION> --kind <KIND>
  starforge-cli repair --session <PATH> --player <PLAYER> --location <LOCATION> --kind <KIND>
  starforge-cli relay --session <PATH> --player <PLAYER> --location <LOCATION> --status <STATUS>
  starforge-cli budget --session <PATH> --player <PLAYER> --upkeep <N> --training <N> --agents <N>
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
  5. `claim` cleared neutral worlds, then expand with `build` and `repair`.
  6. Raise training budget with `budget`, then start `train` for tiers 2 through 5.
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
    New(NewArgs),
    Status(SessionPlayerArgs),
    Map(SessionPlayerArgs),
    Events(EventsArgs),
    Step(StepArgs),
    Survey(TransitArgs),
    Pacify(TransitArgs),
    Claim(TransitArgs),
    Assault(TransitArgs),
    Strike(TransitArgs),
    Build(InfrastructureArgs),
    Repair(InfrastructureArgs),
    Relay(RelayArgs),
    Budget(BudgetArgs),
    Train(TrainArgs),
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct NewArgs {
    #[arg(long, short = 's', value_name = "PATH")]
    pub(crate) session: Option<PathBuf>,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct SessionArg {
    #[arg(long, short = 's', value_name = "PATH")]
    pub(crate) session: PathBuf,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct SessionPlayerArgs {
    #[command(flatten)]
    pub(crate) session: SessionArg,
    #[arg(long, short = 'p', value_name = "PLAYER", value_parser = parse_player_id)]
    pub(crate) player: PlayerId,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct EventsArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[arg(long, default_value_t = 0, value_name = "TICK")]
    pub(crate) from_tick: u64,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct StepArgs {
    #[command(flatten)]
    pub(crate) session: SessionArg,
    #[arg(long, value_name = "TICKS")]
    pub(crate) ticks: u32,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct TransitArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[arg(long, value_name = "LOCATION")]
    pub(crate) origin: u32,
    #[arg(long, value_name = "LOCATION")]
    pub(crate) destination: u32,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct InfrastructureArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[arg(long, value_name = "LOCATION")]
    pub(crate) location: u32,
    #[arg(long, value_name = "KIND", value_parser = parse_infrastructure_kind)]
    pub(crate) kind: InfrastructureKind,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct RelayArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[arg(long, value_name = "LOCATION")]
    pub(crate) location: u32,
    #[arg(long, value_name = "STATUS", value_parser = parse_relay_status)]
    pub(crate) status: RelayStatus,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct BudgetArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[arg(long, value_name = "N")]
    pub(crate) upkeep: u32,
    #[arg(long, value_name = "N")]
    pub(crate) training: u32,
    #[arg(long, value_name = "N")]
    pub(crate) agents: u32,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(crate) struct TrainArgs {
    #[command(flatten)]
    pub(crate) common: SessionPlayerArgs,
    #[arg(long, value_name = "TIER")]
    pub(crate) target_tier: u8,
}

fn parse_player_id(value: &str) -> Result<PlayerId, String> {
    value
        .parse::<u8>()
        .map(PlayerId::new)
        .map_err(|_| format!("invalid player: '{value}'"))
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

    use clap::{Parser, error::ErrorKind};
    use starforge_core::{InfrastructureKind, PlayerId, RelayStatus};

    use super::{
        BudgetArgs, Cli, Command, EventsArgs, InfrastructureArgs, NewArgs, RelayArgs, SessionArg,
        SessionPlayerArgs, StepArgs, TrainArgs, TransitArgs, parse_infrastructure_kind,
        parse_relay_status,
    };

    fn parse_ok(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("arguments should parse successfully")
    }

    fn parse_err(args: &[&str]) -> clap::Error {
        Cli::try_parse_from(args).expect_err("arguments should fail to parse")
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
    fn new_accepts_short_session_alias() {
        assert_eq!(
            parse_ok(&["starforge-cli", "new", "-s", "session.json"]),
            Cli {
                api_base: None,
                command: Command::New(NewArgs {
                    session: Some(PathBuf::from("session.json")),
                }),
            }
        );
    }

    #[test]
    fn status_accepts_long_session_and_player_flags() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "status",
                "--session",
                "session.json",
                "--player",
                "1",
            ]),
            Cli {
                api_base: None,
                command: Command::Status(SessionPlayerArgs {
                    session: SessionArg {
                        session: PathBuf::from("session.json"),
                    },
                    player: PlayerId::new(1),
                }),
            }
        );
    }

    #[test]
    fn status_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&["starforge-cli", "status", "-s", "session.json", "-p", "1"]),
            Cli {
                api_base: None,
                command: Command::Status(SessionPlayerArgs {
                    session: SessionArg {
                        session: PathBuf::from("session.json"),
                    },
                    player: PlayerId::new(1),
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
    fn assault_accepts_transit_arguments() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "assault",
                "--session",
                "session.json",
                "--player",
                "1",
                "--origin",
                "4",
                "--destination",
                "9",
            ]),
            Cli {
                api_base: None,
                command: Command::Assault(TransitArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    origin: 4,
                    destination: 9,
                }),
            }
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
                    origin: 3,
                    destination: 9,
                }),
            }
        );
    }

    #[test]
    fn map_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&["starforge-cli", "map", "-s", "session.json", "-p", "2"]),
            Cli {
                api_base: None,
                command: Command::Map(SessionPlayerArgs {
                    session: SessionArg {
                        session: PathBuf::from("session.json"),
                    },
                    player: PlayerId::new(2),
                }),
            }
        );
    }

    #[test]
    fn events_defaults_from_tick_to_zero() {
        assert_eq!(
            parse_ok(&["starforge-cli", "events", "-s", "session.json", "-p", "1"]),
            Cli {
                api_base: None,
                command: Command::Events(EventsArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    from_tick: 0,
                }),
            }
        );
    }

    #[test]
    fn events_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "events",
                "-s",
                "session.json",
                "-p",
                "1",
                "--from-tick",
                "4",
            ]),
            Cli {
                api_base: None,
                command: Command::Events(EventsArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    from_tick: 4,
                }),
            }
        );
    }

    #[test]
    fn step_accepts_short_session_alias() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "step",
                "-s",
                "session.json",
                "--ticks",
                "3"
            ]),
            Cli {
                api_base: None,
                command: Command::Step(StepArgs {
                    session: SessionArg {
                        session: PathBuf::from("session.json"),
                    },
                    ticks: 3,
                }),
            }
        );
    }

    #[test]
    fn survey_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "survey",
                "-s",
                "session.json",
                "-p",
                "1",
                "--origin",
                "7",
                "--destination",
                "9",
            ]),
            Cli {
                api_base: None,
                command: Command::Survey(TransitArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    origin: 7,
                    destination: 9,
                }),
            }
        );
    }

    #[test]
    fn pacify_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "pacify",
                "-s",
                "session.json",
                "-p",
                "1",
                "--origin",
                "7",
                "--destination",
                "9",
            ]),
            Cli {
                api_base: None,
                command: Command::Pacify(TransitArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    origin: 7,
                    destination: 9,
                }),
            }
        );
    }

    #[test]
    fn claim_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "claim",
                "-s",
                "session.json",
                "-p",
                "1",
                "--origin",
                "7",
                "--destination",
                "9",
            ]),
            Cli {
                api_base: None,
                command: Command::Claim(TransitArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    origin: 7,
                    destination: 9,
                }),
            }
        );
    }

    #[test]
    fn build_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "build",
                "-s",
                "session.json",
                "-p",
                "1",
                "--location",
                "5",
                "--kind",
                "command-nexus",
            ]),
            Cli {
                api_base: None,
                command: Command::Build(InfrastructureArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    location: 5,
                    kind: InfrastructureKind::CommandNexus,
                }),
            }
        );
    }

    #[test]
    fn repair_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "repair",
                "-s",
                "session.json",
                "-p",
                "1",
                "--location",
                "5",
                "--kind",
                "datacenter",
            ]),
            Cli {
                api_base: None,
                command: Command::Repair(InfrastructureArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    location: 5,
                    kind: InfrastructureKind::Datacenter,
                }),
            }
        );
    }

    #[test]
    fn relay_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "relay",
                "-s",
                "session.json",
                "-p",
                "1",
                "--location",
                "5",
                "--status",
                "disconnected",
            ]),
            Cli {
                api_base: None,
                command: Command::Relay(RelayArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    location: 5,
                    status: RelayStatus::Disconnected,
                }),
            }
        );
    }

    #[test]
    fn budget_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "budget",
                "-s",
                "session.json",
                "-p",
                "1",
                "--upkeep",
                "10",
                "--training",
                "20",
                "--agents",
                "30",
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
                    upkeep: 10,
                    training: 20,
                    agents: 30,
                }),
            }
        );
    }

    #[test]
    fn train_accepts_short_session_and_player_aliases() {
        assert_eq!(
            parse_ok(&[
                "starforge-cli",
                "train",
                "-s",
                "session.json",
                "-p",
                "1",
                "--target-tier",
                "4",
            ]),
            Cli {
                api_base: None,
                command: Command::Train(TrainArgs {
                    common: SessionPlayerArgs {
                        session: SessionArg {
                            session: PathBuf::from("session.json"),
                        },
                        player: PlayerId::new(1),
                    },
                    target_tier: 4,
                }),
            }
        );
    }

    #[test]
    fn legacy_positional_status_syntax_is_rejected() {
        let error = parse_err(&["starforge-cli", "status", "session.json", "1"]);
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("session.json"));
    }

    #[test]
    fn legacy_positional_survey_syntax_is_rejected() {
        let error = parse_err(&["starforge-cli", "survey", "session.json", "1", "2", "3"]);
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("session.json"));
    }

    #[test]
    fn unsupported_short_alias_for_ticks_is_rejected() {
        let error = parse_err(&["starforge-cli", "step", "-s", "session.json", "-t", "3"]);
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("-t"));
    }

    #[test]
    fn unsupported_short_alias_for_target_tier_is_rejected() {
        let error = parse_err(&[
            "starforge-cli",
            "train",
            "-s",
            "session.json",
            "-p",
            "1",
            "-t",
            "3",
        ]);
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("-t"));
    }

    #[test]
    fn unsupported_short_alias_for_origin_is_rejected() {
        let error = parse_err(&[
            "starforge-cli",
            "survey",
            "-s",
            "session.json",
            "-p",
            "1",
            "-o",
            "1",
            "--destination",
            "2",
        ]);
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("-o"));
    }

    #[test]
    fn infrastructure_kind_parser_accepts_existing_alias_forms() {
        assert_eq!(
            parse_infrastructure_kind("CommandNexus"),
            Ok(InfrastructureKind::CommandNexus)
        );
        assert_eq!(
            parse_infrastructure_kind("command_nexus"),
            Ok(InfrastructureKind::CommandNexus)
        );
        assert_eq!(
            parse_infrastructure_kind("command-nexus"),
            Ok(InfrastructureKind::CommandNexus)
        );
    }

    #[test]
    fn relay_status_parser_accepts_connected_and_disconnected() {
        assert_eq!(parse_relay_status("connected"), Ok(RelayStatus::Connected));
        assert_eq!(
            parse_relay_status("disconnected"),
            Ok(RelayStatus::Disconnected)
        );
    }
}
