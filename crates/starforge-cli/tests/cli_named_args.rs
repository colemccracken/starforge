use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    thread,
    time::Duration,
};

use assert_cmd::Command;
use predicates::prelude::*;
use starforge_api::ApiServerConfig;
use starforge_core::SessionId;
use starforge_scenarios::starter_skirmish_harness;
use tempfile::tempdir;

fn cli_command() -> Command {
    Command::cargo_bin("starforge-cli").expect("starforge-cli binary should build")
}

static API_SERVER_TEST_MUTEX: Mutex<()> = Mutex::new(());

struct SpawnedApiServer {
    base_url: String,
    _guard: MutexGuard<'static, ()>,
}

fn seed_session(path: &Path) {
    let harness = starter_skirmish_harness().expect("starter harness should load");
    let session = harness.instantiate_session(SessionId::new(1));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("session parent directory should be created");
    }

    fs::write(
        path,
        session.snapshot_json().expect("snapshot should serialize"),
    )
    .expect("session file should be written");
}

fn temp_session_path() -> (tempfile::TempDir, PathBuf) {
    let temp = tempdir().expect("tempdir should be created");
    let session_path = temp.path().join("session.json");
    (temp, session_path)
}

fn spawn_api_server() -> SpawnedApiServer {
    let guard = API_SERVER_TEST_MUTEX
        .lock()
        .expect("api server test mutex should not be poisoned");
    let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral port should bind");
    let address = listener.local_addr().expect("local address should resolve");
    drop(listener);

    let bind_address = format!("127.0.0.1:{}", address.port());
    let base_url = format!("http://{bind_address}");
    thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");
        runtime
            .block_on(starforge_api::run_server(ApiServerConfig {
                bind_address,
                ..ApiServerConfig::default()
            }))
            .expect("api server should run");
    });
    thread::sleep(Duration::from_millis(200));
    SpawnedApiServer {
        base_url,
        _guard: guard,
    }
}

#[test]
fn help_shows_named_flags_and_only_s_p_short_aliases() {
    cli_command()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--session"))
        .stdout(predicate::str::contains("--player"))
        .stdout(predicate::str::contains("-s"))
        .stdout(predicate::str::contains("-p"))
        .stdout(predicate::str::contains("--ticks"))
        .stdout(predicate::str::contains("--origin"))
        .stdout(predicate::str::contains("--destination"))
        .stdout(predicate::str::contains("--target-tier"))
        .stdout(predicate::str::contains("status <session_path> <player_id>").not())
        .stdout(predicate::str::contains("-t,").not())
        .stdout(predicate::str::contains("-o,").not())
        .stdout(predicate::str::contains("-d,").not());
}

#[test]
fn new_with_short_session_alias_creates_session() {
    let (_temp, session_path) = temp_session_path();

    cli_command()
        .args(["new", "-s"])
        .arg(&session_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Created session at"));

    assert!(session_path.exists(), "session file should exist");
}

#[test]
fn new_prints_named_suggestions() {
    let temp = tempdir().expect("tempdir should be created");

    cli_command()
        .current_dir(temp.path())
        .arg("new")
        .assert()
        .success()
        .stdout(predicate::str::contains("starforge-cli map --session"))
        .stdout(predicate::str::contains("starforge-cli status --session"))
        .stdout(predicate::str::contains("--player 1"))
        .stdout(predicate::str::contains("--origin"))
        .stdout(predicate::str::contains("--destination"))
        .stdout(predicate::str::contains("--ticks"));

    assert!(
        temp.path().join("starforge-session.json").exists(),
        "default session file should exist"
    );
}

#[test]
fn status_works_with_short_aliases() {
    let (_temp, session_path) = temp_session_path();
    seed_session(&session_path);

    cli_command()
        .args(["status", "-s"])
        .arg(&session_path)
        .args(["-p", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Player: P1"));
}

#[test]
fn step_and_events_work_with_mixed_short_and_long_flags() {
    let (_temp, session_path) = temp_session_path();
    seed_session(&session_path);

    cli_command()
        .args(["step", "-s"])
        .arg(&session_path)
        .args(["--ticks", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Advanced to tick 1"));

    cli_command()
        .args(["events", "-s"])
        .arg(&session_path)
        .args(["-p", "1", "--from-tick", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[1] tick advanced to 1"));
}

#[test]
fn legacy_positional_invocation_fails() {
    cli_command()
        .args(["status", "session.json", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"))
        .stderr(predicate::str::contains("session.json"));
}

#[test]
fn unsupported_short_alias_invocation_fails() {
    cli_command()
        .args(["step", "-s", "session.json", "-t", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"))
        .stderr(predicate::str::contains("-t"));
}

#[test]
fn api_mode_can_create_and_query_a_remote_session() {
    let api = spawn_api_server();

    cli_command()
        .args(["--api-base", &api.base_url, "new"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created remote session #1"));

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "status",
            "--session",
            "1",
            "--player",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Session: #1 @"))
        .stdout(predicate::str::contains("Player: P1"));
}

#[test]
fn api_mode_map_shows_known_routes() {
    let api = spawn_api_server();

    cli_command()
        .args(["--api-base", &api.base_url, "new"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created remote session #1"));

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "map",
            "--session",
            "1",
            "--player",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Reachable routes from currently known worlds:",
        ))
        .stdout(predicate::str::contains("<->"))
        .stdout(predicate::str::contains("unavailable via API-backed CLI").not());
}

#[test]
fn api_mode_run_and_pause_commands_work() {
    let api = spawn_api_server();

    cli_command()
        .args(["--api-base", &api.base_url, "new"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created remote session #1"));

    cli_command()
        .args(["--api-base", &api.base_url, "run", "--session", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Session #1 is running"));

    thread::sleep(Duration::from_millis(150));

    cli_command()
        .args(["--api-base", &api.base_url, "pause", "--session", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Session #1 is paused"));

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "status",
            "--session",
            "1",
            "--player",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Control: paused"));
}

#[test]
fn file_mode_metrics_save_load_and_scenario_run_work() {
    let (_temp, session_path) = temp_session_path();
    seed_session(&session_path);
    let snapshot_path = session_path.with_file_name("session-snapshot.json");
    let restored_path = session_path.with_file_name("restored-session.json");
    let scenario_path = session_path.with_file_name("scenario-session.json");

    cli_command()
        .args(["metrics", "--session"])
        .arg(&session_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Control: paused"))
        .stdout(predicate::str::contains("Accepted commands:"));

    cli_command()
        .args(["save", "--session"])
        .arg(&session_path)
        .args(["--output"])
        .arg(&snapshot_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Saved snapshot"));

    assert!(snapshot_path.exists(), "snapshot file should exist");

    cli_command()
        .args(["load", "--input"])
        .arg(&snapshot_path)
        .args(["--session"])
        .arg(&restored_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Loaded session"));

    assert!(restored_path.exists(), "restored session file should exist");

    cli_command()
        .args(["scenario-run", "--session"])
        .arg(&scenario_path)
        .args(["--ticks", "3"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created scenario session at"))
        .stdout(predicate::str::contains(
            "Advanced scenario session to tick 3",
        ));

    assert!(scenario_path.exists(), "scenario session file should exist");
}

#[test]
fn api_mode_metrics_save_and_load_work() {
    let api = spawn_api_server();
    let temp = tempdir().expect("tempdir should be created");
    let snapshot_path = temp.path().join("remote-snapshot.json");

    cli_command()
        .args(["--api-base", &api.base_url, "new"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created remote session #1"));

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "step",
            "--session",
            "1",
            "--ticks",
            "3",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Advanced to tick 3"));

    cli_command()
        .args(["--api-base", &api.base_url, "metrics", "--session", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tick: 3"))
        .stdout(predicate::str::contains("Accepted commands:"));

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "save",
            "--session",
            "1",
            "--output",
        ])
        .arg(&snapshot_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Saved remote session #1"));

    assert!(snapshot_path.exists(), "remote snapshot file should exist");

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "step",
            "--session",
            "1",
            "--ticks",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Advanced to tick 5"));

    cli_command()
        .args(["--api-base", &api.base_url, "load", "--input"])
        .arg(&snapshot_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Loaded remote session #1"))
        .stdout(predicate::str::contains("tick 3"));

    cli_command()
        .args([
            "--api-base",
            &api.base_url,
            "status",
            "--session",
            "1",
            "--player",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tick: 3"));
}
