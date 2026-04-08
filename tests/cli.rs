use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn isolated_env() -> TempDir {
    TempDir::new().unwrap()
}

#[test]
fn agent_info_returns_valid_json() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).expect("agent-info must be JSON");
    assert_eq!(value["name"], "mailing-list-cli");
    assert!(value["commands"].is_object());
    assert!(
        value["exit_codes"]["2"]
            .as_str()
            .unwrap()
            .contains("Config")
    );
}

#[test]
fn agent_info_lists_health_command() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert!(value["commands"]["health"].is_string());
}

#[test]
fn agent_info_lists_phase_3_commands() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&stdout).unwrap();
    let commands = v["commands"].as_object().unwrap();
    // Sanity: every major Phase 3 command is advertised
    for key in [
        "contact show <email>",
        "contact tag <email> <tag>",
        "contact import <file.csv> --list <id> [--unsafe-no-consent]",
        "tag ls",
        "field create <key> --type <text|number|date|bool|select> [--options a,b,c]",
        "segment create <name> --filter <expr>",
        "segment members <name> [--limit N] [--cursor C]",
    ] {
        assert!(
            commands.contains_key(key),
            "agent-info missing command: {key}"
        );
    }
}

#[test]
fn version_flag_exits_zero() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mailing-list-cli"));
}

#[test]
fn help_flag_exits_zero() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("mailing list management"));
}

#[test]
fn health_with_stub_email_cli_succeeds() {
    let stub = fixture_path("stub-email-cli.sh");
    assert!(stub.exists(), "stub-email-cli.sh must exist at {:?}", stub);

    let tmp = isolated_env();
    let config_path = tmp.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
[sender]
physical_address = "123 Test St"

[email_cli]
path = "{}"
profile = "default"
"#,
            stub.display()
        ),
    )
    .unwrap();

    let db_path = tmp.path().join("state.db");

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "health"]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["status"], "success");
    assert_eq!(value["data"]["status"], "ok");
}

#[test]
fn health_without_email_cli_fails_with_exit_2() {
    let tmp = isolated_env();
    let config_path = tmp.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"
[sender]
physical_address = "123 Test St"

[email_cli]
path = "/definitely/not/a/real/path/email-cli"
profile = "default"
"#,
    )
    .unwrap();
    let db_path = tmp.path().join("state.db");

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "health"]);
    cmd.assert().failure().code(2);
}

#[test]
fn unknown_command_returns_failure() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.arg("definitely-not-a-real-subcommand")
        .assert()
        .failure();
}

fn stub_env() -> (TempDir, PathBuf, PathBuf) {
    let stub = fixture_path("stub-email-cli.sh");
    assert!(stub.exists(), "stub-email-cli.sh must exist");
    let tmp = isolated_env();
    let config_path = tmp.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
[sender]
from = "test@example.com"
physical_address = "123 Test St"

[email_cli]
path = "{}"
profile = "default"

[unsubscribe]
public_url = "https://hooks.example.com/u"
secret_env = "MLC_UNSUBSCRIBE_SECRET"
"#,
            stub.display()
        ),
    )
    .unwrap();
    let db_path = tmp.path().join("state.db");
    (tmp, config_path, db_path)
}

#[test]
fn list_create_then_list_ls_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // Create the list
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "newsletter"]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["status"], "success");
    assert_eq!(value["data"]["name"], "newsletter");
    assert_eq!(value["data"]["resend_segment_id"], "seg_test_12345");

    // List them
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "ls"]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["data"]["count"], 1);
    assert_eq!(value["data"]["lists"][0]["name"], "newsletter");
}

#[test]
fn list_create_duplicate_returns_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "dup"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "dup"]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_add_then_contact_ls_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // Create a list first
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "newsletter"]);
    cmd.assert().success();

    // Add a contact
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
            "--first-name",
            "Alice",
        ]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["data"]["email"], "alice@example.com");
    assert_eq!(value["data"]["list_id"], 1);

    // List contacts
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--list", "1"]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["data"]["count"], 1);
    assert_eq!(value["data"]["contacts"][0]["email"], "alice@example.com");
    assert_eq!(value["data"]["contacts"][0]["first_name"], "Alice");
}

#[test]
fn contact_add_to_unknown_list_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "999",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_add_invalid_email_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "add", "not-an-email", "--list", "1"]);
    cmd.assert().failure().code(3);
}

#[test]
fn tag_ls_on_empty_db_returns_count_zero() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "ls"]);
    let out = cmd.assert().success();
    let value: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(value["data"]["count"], 0);
}

#[test]
fn tag_rm_without_confirm_fails() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "rm", "vip"]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_tag_and_untag_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // seed: list + contact
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ]);
    cmd.assert().success();

    // tag
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "tag", "alice@example.com", "vip"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["tag"], "vip");

    // tag ls must show 1 member
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["tags"][0]["member_count"], 1);

    // untag
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "untag", "alice@example.com", "vip"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["removed"], true);
}

#[test]
fn contact_tag_on_missing_contact_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "tag", "ghost@example.com", "vip"]);
    cmd.assert().failure().code(3);
}

#[test]
fn field_create_list_rm_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // Create
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "company", "--type", "text"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["field"]["key"], "company");
    assert_eq!(v["data"]["field"]["type"], "text");

    // List
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);

    // Rm without --confirm fails
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "rm", "company"]);
    cmd.assert().failure().code(3);

    // Rm with --confirm succeeds
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "rm", "company", "--confirm"]);
    cmd.assert().success();
}

#[test]
fn field_create_select_without_options_fails() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "plan", "--type", "select"]);
    cmd.assert().failure().code(3);
}

#[test]
fn field_create_select_with_options_succeeds() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "field",
            "create",
            "plan",
            "--type",
            "select",
            "--options",
            "free,pro,enterprise",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["field"]["options"][1], "pro");
}

#[test]
fn contact_set_with_typed_field_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // list + contact + field
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "age", "--type", "number"]);
    cmd.assert().success();

    // set numeric value
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "set", "alice@example.com", "age", "42"]);
    cmd.assert().success();

    // rejecting non-numeric value
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "set",
            "alice@example.com",
            "age",
            "old",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_add_with_field_flags() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "company", "--type", "text"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
            "--field",
            "company=Acme",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["fields_set"], 1);
}

#[test]
fn contact_show_returns_full_details() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed
    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "field", "create", "company", "--type", "text"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
            "--first-name",
            "Alice",
            "--field",
            "company=Acme",
        ],
        vec!["--json", "contact", "tag", "alice@example.com", "vip"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Show
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "alice@example.com"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["contact"]["email"], "alice@example.com");
    assert_eq!(v["data"]["contact"]["first_name"], "Alice");
    assert_eq!(v["data"]["tags"][0], "vip");
    assert_eq!(v["data"]["fields"]["company"], "Acme");
    assert_eq!(v["data"]["lists"][0]["name"], "news");
}

#[test]
fn contact_show_on_missing_email_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "ghost@example.com"]);
    cmd.assert().failure().code(3);
}

#[test]
fn segment_create_list_show_members_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed: list, contact, tag
    for args in [
        vec!["--json", "list", "create", "news"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ],
        vec!["--json", "contact", "tag", "alice@example.com", "vip"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Create segment
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "create", "vips", "--filter", "tag:vip"]);
    cmd.assert().success();

    // List
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["segments"][0]["member_count"], 1);

    // Show
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "show", "vips"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["member_count"], 1);
    assert_eq!(v["data"]["sample"][0]["email"], "alice@example.com");

    // Members
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "members", "vips"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["contacts"][0]["email"], "alice@example.com");

    // Rm without --confirm fails
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "rm", "vips"]);
    cmd.assert().failure().code(3);

    // Rm with --confirm succeeds
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "rm", "vips", "--confirm"]);
    cmd.assert().success();
}

#[test]
fn segment_create_with_invalid_filter_returns_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "segment",
            "create",
            "bad",
            "--filter",
            "((unclosed",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_ls_with_filter_returns_matching_subset() {
    let (_tmp, config_path, db_path) = stub_env();

    for args in [
        vec!["--json", "list", "create", "news"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ],
        vec!["--json", "contact", "add", "bob@example.com", "--list", "1"],
        vec!["--json", "contact", "tag", "alice@example.com", "vip"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--filter", "tag:vip"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["contacts"][0]["email"], "alice@example.com");
}

#[test]
fn contact_ls_with_cursor_paginates() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed: 3 contacts
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();
    for email in ["a@ex.com", "b@ex.com", "c@ex.com"] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(["--json", "contact", "add", email, "--list", "1"]);
        cmd.assert().success();
    }

    // First page: limit 2
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--limit", "2"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 2);
    let cursor = v["data"]["next_cursor"].as_i64().unwrap();

    // Second page
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "ls",
            "--limit",
            "2",
            "--cursor",
            &cursor.to_string(),
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["contacts"][0]["email"], "c@ex.com");
}

#[test]
fn segment_members_matches_contact_ls_filter() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed a non-trivial dataset: 4 contacts, 2 tags, varied memberships
    for args in [
        vec!["--json", "list", "create", "news"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@ex.com",
            "--list",
            "1",
            "--first-name",
            "Alice",
        ],
        vec!["--json", "contact", "add", "bob@ex.com", "--list", "1"],
        vec!["--json", "contact", "add", "carol@ex.com", "--list", "1"],
        vec!["--json", "contact", "add", "dan@ex.com", "--list", "1"],
        vec!["--json", "contact", "tag", "alice@ex.com", "vip"],
        vec!["--json", "contact", "tag", "carol@ex.com", "vip"],
        vec!["--json", "contact", "tag", "alice@ex.com", "early"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    let filter = "tag:vip AND NOT tag:early";

    // Path 1: contact ls --filter
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--filter", filter]);
    let out = cmd.assert().success();
    let v_ls: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();

    // Path 2: segment create + segment members
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "create", "loyal", "--filter", filter]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "members", "loyal"]);
    let out = cmd.assert().success();
    let v_seg: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();

    // Emails should match exactly
    let ls_emails: Vec<String> = v_ls["data"]["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["email"].as_str().unwrap().to_string())
        .collect();
    let seg_emails: Vec<String> = v_seg["data"]["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["email"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ls_emails, seg_emails);
    assert_eq!(ls_emails, vec!["carol@ex.com".to_string()]);
}

#[test]
fn contact_import_happy_path() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("contacts.csv");
    std::fs::write(
        &csv_path,
        "email,first_name,consent_source\n\
         alice@example.com,Alice,landing\n\
         bob@example.com,Bob,manual\n",
    )
    .unwrap();

    // Create list first
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    // Import
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["inserted"], 2);
    assert_eq!(v["data"]["skipped_suppressed"], 0);
}

#[test]
fn contact_import_rejects_missing_consent_source() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("nocnt.csv");
    std::fs::write(&csv_path, "email,first_name\nalice@example.com,Alice\n").unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_import_unsafe_no_consent_tags_rows() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("nocnt.csv");
    std::fs::write(&csv_path, "email\nalice@example.com\n").unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
            "--unsafe-no-consent",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["tagged_without_consent"], 1);

    // And the tag actually landed
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let tags: Vec<String> = v["data"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(tags.contains(&"imported_without_consent".to_string()));
}

#[test]
fn contact_import_rejects_double_opt_in_flag_in_phase_3() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("doi.csv");
    std::fs::write(
        &csv_path,
        "email,consent_source\nalice@example.com,manual\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
            "--double-opt-in",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_import_rerun_is_idempotent() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("contacts.csv");
    std::fs::write(
        &csv_path,
        "email,consent_source\nalice@example.com,manual\nbob@example.com,manual\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    for _ in 0..3 {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args([
                "--json",
                "contact",
                "import",
                csv_path.to_str().unwrap(),
                "--list",
                "1",
            ]);
        cmd.assert().success();
    }

    // Still exactly 2 contacts
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--list", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 2);
}

#[test]
fn contact_add_duplicate_triggers_segment_contact_add() {
    let (_tmp, config_path, db_path) = stub_env();

    // Create list
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    // Simulate duplicate on the Resend side; the local DB should still succeed
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_STUB_CONTACT_DUPLICATE", "1")
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ]);
    cmd.assert().success();

    // Verify local membership
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--list", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
}

#[test]
fn contact_ls_filter_on_date_field_queries_correct_column() {
    // Bug 1 regression: date fields must be queried via `value_date`, not
    // `value_text` (the previous behavior) or `value_number`. Prior to the
    // fix, `event_date:>:2026-01-01` silently returned zero rows because the
    // compiler picked `value_text` for the inequality.
    let (_tmp, config_path, db_path) = stub_env();

    // Seed: list + contact + date field + date value
    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "field", "create", "event_date", "--type", "date"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ],
        vec![
            "--json",
            "contact",
            "set",
            "alice@example.com",
            "event_date",
            "2026-03-15T00:00:00Z",
        ],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Filter with a date comparison should find alice
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "ls",
            "--filter",
            "event_date:>:2026-01-01",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1, "expected date filter to match alice");
    assert_eq!(v["data"]["contacts"][0]["email"], "alice@example.com");

    // And a comparison that excludes alice
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "ls",
            "--filter",
            "event_date:>:2030-01-01",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 0);
}

#[test]
fn contact_import_persists_consent_source() {
    // Bug 3 regression: the CSV importer previously threw away the
    // consent_source value. After the fix, importing a row with
    // `consent_source=landing` must persist it on the contact row.
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("consent.csv");
    std::fs::write(
        &csv_path,
        "email,consent_source\nalice@example.com,landing-page\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
        ]);
    cmd.assert().success();

    // `contact show` reveals the stored consent via data.consent
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "alice@example.com"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["consent"]["source"], "landing-page");
    assert!(
        v["data"]["consent"]["at"].as_str().is_some(),
        "consent.at must be populated"
    );

    // The `imported_without_consent` auto-tag must NOT be present — the
    // row had a real consent_source.
    let tags = v["data"]["tags"].as_array().unwrap();
    assert!(
        !tags
            .iter()
            .any(|t| t.as_str() == Some("imported_without_consent"))
    );
}

#[test]
fn contact_import_unsafe_preserves_existing_consent() {
    // Bug 3 regression: a previously consented contact must not have
    // their consent overwritten (nor be auto-tagged
    // `imported_without_consent`) by a later --unsafe-no-consent import.
    let (tmp, config_path, db_path) = stub_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    // First import: real consent source
    let first_csv = tmp.path().join("first.csv");
    std::fs::write(
        &first_csv,
        "email,consent_source\nalice@example.com,landing-page\n",
    )
    .unwrap();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            first_csv.to_str().unwrap(),
            "--list",
            "1",
        ]);
    cmd.assert().success();

    // Second import: --unsafe-no-consent with only email
    let second_csv = tmp.path().join("second.csv");
    std::fs::write(&second_csv, "email\nalice@example.com\n").unwrap();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            second_csv.to_str().unwrap(),
            "--list",
            "1",
            "--unsafe-no-consent",
        ]);
    cmd.assert().success();

    // Consent is still `landing-page` (not overwritten) and the
    // imported_without_consent tag is NOT applied.
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "alice@example.com"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["consent"]["source"], "landing-page");
    let tags = v["data"]["tags"].as_array().unwrap();
    assert!(
        !tags
            .iter()
            .any(|t| t.as_str() == Some("imported_without_consent")),
        "unsafe import must not retroactively tag a consented contact"
    );
}

#[test]
fn contact_import_rolls_back_on_field_error() {
    // Bug 2 regression: apply_row_local used to call contact_upsert and
    // contact_add_to_list before validating custom-field values. When a
    // later field coercion failed, the contact row and list membership
    // had already been written, leaving half-imported state. After the
    // fix, a row with an invalid field must leave NO trace (but valid
    // rows imported alongside it must still land).
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("mixed.csv");
    std::fs::write(
        &csv_path,
        "email,consent_source,age\n\
         alice@example.com,manual,30\n\
         bob@example.com,manual,not-a-number\n",
    )
    .unwrap();

    // list + number field
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "age", "--type", "number"]);
    cmd.assert().success();

    // Import: alice should succeed, bob should fail on the `age` coercion.
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["inserted"], 1);
    assert_eq!(v["data"]["skipped_invalid"], 1);

    // Alice is present, bob is not (no half-written contact row).
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--list", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let emails: Vec<String> = v["data"]["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["email"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(emails, vec!["alice@example.com".to_string()]);

    // And contact show bob@example.com should fail: the row never existed.
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "bob@example.com"]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_ls_filter_on_text_field_with_numeric_content() {
    // Bug 1 regression: a text field that contains numeric-looking content
    // like "00123" must be queried via `value_text`, not `value_number`.
    let (_tmp, config_path, db_path) = stub_env();

    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "field", "create", "zip_code", "--type", "text"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
        ],
        vec![
            "--json",
            "contact",
            "set",
            "alice@example.com",
            "zip_code",
            "00123",
        ],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Equality on the zero-padded zip must match; the old path queried
    // value_number which holds NULL for text fields.
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--filter", "zip_code:=:00123"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["contacts"][0]["email"], "alice@example.com");
}

const VALID_TEMPLATE: &str = r#"---
name: welcome
subject: "Welcome, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Welcome</mj-title>
    <mj-preview>Welcome to our list</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

fn write_template_file(tmp: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = tmp.path().join(format!("{name}.mjml.hbs"));
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn template_create_from_file_list_show_round_trip() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);

    // Create from file
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "template",
            "create",
            "welcome",
            "--from-file",
            template_path.to_str().unwrap(),
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["name"], "welcome");
    assert_eq!(v["data"]["subject"], "Welcome, {{ first_name }}");

    // List
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);

    // Show
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "show", "welcome"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert!(v["data"]["mjml_source"].as_str().unwrap().contains("mjml"));
}

#[test]
fn template_create_scaffold_without_file() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "create", "scaffold"]);
    cmd.assert().success();
}

#[test]
fn template_render_with_data_returns_html() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "template",
            "create",
            "welcome",
            "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let data_path = tmp.path().join("data.json");
    std::fs::write(&data_path, r#"{"first_name":"Alice"}"#).unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "template",
            "render",
            "welcome",
            "--with-data",
            data_path.to_str().unwrap(),
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["subject"], "Welcome, Alice");
    assert!(v["data"]["html"].as_str().unwrap().contains("Hi Alice"));
    assert!(v["data"]["size_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn template_lint_clean_template_passes() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "template",
            "create",
            "welcome",
            "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "lint", "welcome"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["errors"], 0);
}

#[test]
fn template_lint_missing_unsubscribe_errors() {
    let (tmp, config_path, db_path) = stub_env();
    let bad = VALID_TEMPLATE
        .replace("{{{ unsubscribe_link }}}", "")
        .replace("name: welcome", "name: bad");
    let template_path = write_template_file(&tmp, "bad", &bad);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "template",
            "create",
            "bad",
            "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "lint", "bad"]);
    cmd.assert().failure().code(3);
}

#[test]
fn template_rm_without_confirm_fails() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "template",
            "create",
            "welcome",
            "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "rm", "welcome"]);
    cmd.assert().failure().code(3);
}

#[test]
fn template_guidelines_prints_authoring_guide() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.args(["--json", "template", "guidelines"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let guide = v["data"]["guide_markdown"].as_str().unwrap();
    assert!(guide.contains("Template Authoring for mailing-list-cli"));
    assert!(guide.contains("{{{ unsubscribe_link }}}"));
}

#[test]
fn agent_info_lists_phase_4_commands() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let commands = v["commands"].as_object().unwrap();
    for key in [
        "template create <name> [--from-file <path>]",
        "template render <name> [--with-data <file.json>] [--with-placeholders]",
        "template lint <name>",
        "template guidelines",
    ] {
        assert!(commands.contains_key(key), "agent-info missing {key}");
    }
}

const SIMPLE_TEMPLATE: &str = r#"---
name: simple_ad
subject: "Hi {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Hi</mj-title>
    <mj-preview>Hello there</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        <mj-button href="https://example.com/cta">Click me</mj-button>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

fn seed_broadcast_env() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let (tmp, config_path, db_path) = stub_env();
    // Per-test cache dir to avoid polluting the user's cache during tests.
    let cache_dir = tmp.path().join("cache");
    // Create list + contact + template
    let template_path = tmp.path().join("simple.mjml.hbs");
    std::fs::write(&template_path, SIMPLE_TEMPLATE).unwrap();

    for args in [
        vec!["--json", "list", "create", "news"],
        vec![
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
            "--first-name",
            "Alice",
        ],
        vec![
            "--json",
            "template",
            "create",
            "simple_ad",
            "--from-file",
            template_path.to_str().unwrap(),
        ],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .env("MLC_CACHE_DIR", &cache_dir)
            .args(&args);
        cmd.assert().success();
    }
    (tmp, config_path, db_path, cache_dir)
}

#[test]
fn broadcast_create_list_target_and_show() {
    let (_tmp, config_path, db_path, cache_dir) = seed_broadcast_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args([
            "--json",
            "broadcast",
            "create",
            "--name",
            "Q1 ad",
            "--template",
            "simple_ad",
            "--to",
            "list:news",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["broadcast"]["name"], "Q1 ad");
    assert_eq!(v["data"]["broadcast"]["status"], "draft");

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args(["--json", "broadcast", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
}

#[test]
fn broadcast_send_via_stub_updates_status_to_sent() {
    let (_tmp, config_path, db_path, cache_dir) = seed_broadcast_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args([
            "--json",
            "broadcast",
            "create",
            "--name",
            "test",
            "--template",
            "simple_ad",
            "--to",
            "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .env("MLC_UNSUBSCRIBE_SECRET", "test-secret-long-enough")
        .args(["--json", "broadcast", "send", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["sent"], 1);

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args(["--json", "broadcast", "show", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["broadcast"]["status"], "sent");
}

#[test]
fn broadcast_cancel_without_confirm_fails() {
    let (_tmp, config_path, db_path, cache_dir) = seed_broadcast_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args([
            "--json",
            "broadcast",
            "create",
            "--name",
            "test",
            "--template",
            "simple_ad",
            "--to",
            "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args(["--json", "broadcast", "cancel", "1"]);
    cmd.assert().failure().code(3);
}

#[test]
fn broadcast_preview_via_stub_sends_single() {
    let (_tmp, config_path, db_path, cache_dir) = seed_broadcast_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args([
            "--json",
            "broadcast",
            "create",
            "--name",
            "test",
            "--template",
            "simple_ad",
            "--to",
            "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .env("MLC_UNSUBSCRIBE_SECRET", "test-secret-long-enough")
        .args([
            "--json",
            "broadcast",
            "preview",
            "1",
            "--to",
            "preview@example.com",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["to"], "preview@example.com");
}

// ─── Phase 6: webhook + report tests ──────────────────────────────────────

#[test]
fn event_poll_with_no_events_returns_zero_processed() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "event", "poll"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["status"], "success");
    assert_eq!(v["data"]["processed"].as_i64().unwrap(), 0);
    assert_eq!(v["data"]["duplicates"].as_i64().unwrap(), 0);
}

#[test]
fn event_poll_processes_synthetic_delivered_event_from_stub() {
    let (_tmp, config_path, db_path) = stub_env();

    // Feed a synthetic delivered email to the stub via env var. The event has
    // no matching broadcast_recipient, so handle_event records the event row
    // but does not increment any broadcast counters — that is fine for this
    // test, which only verifies that event poll → handle_event → event_insert
    // is wired end-to-end.
    let stub_response = r#"{"version":"1","status":"success","data":{"object":"list","has_more":false,"data":[{"id":"em_stub_deliver","to":["alice@example.com"],"last_event":"delivered","created_at":"2026-04-08T12:00:00Z","subject":"Hi"}]}}"#;

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_STUB_EMAIL_LIST_JSON", stub_response)
        .args(["--json", "event", "poll"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["processed"].as_i64().unwrap(), 1);
    assert_eq!(v["data"]["duplicates"].as_i64().unwrap(), 0);

    // Polling again with the same payload must be idempotent. The cursor was
    // advanced past em_stub_deliver, but the stub doesn't honour the cursor,
    // so the same event is offered again — the unique index on
    // (resend_email_id, type) means it dedupes to a duplicate.
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_STUB_EMAIL_LIST_JSON", stub_response)
        .args(["--json", "event", "poll"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["processed"].as_i64().unwrap(), 0);
    assert_eq!(v["data"]["duplicates"].as_i64().unwrap(), 1);
}

#[test]
fn event_poll_after_broadcast_send_updates_delivered_count_and_report_show() {
    // End-to-end: seed list/contact/template, send a broadcast through the stub
    // batch send (which assigns resend_email_id = "em_stub_1"), then feed a
    // delivered event for em_stub_1 back through `event poll` and verify
    // the broadcast's delivered_count is incremented and `report show` reports
    // a sensible CTR / bounce_rate envelope.
    let (_tmp, config_path, db_path, cache_dir) = seed_broadcast_env();

    // Create + send the broadcast
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args([
            "--json",
            "broadcast",
            "create",
            "--name",
            "phase6-report",
            "--template",
            "simple_ad",
            "--to",
            "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .env("MLC_UNSUBSCRIBE_SECRET", "test-secret-long-enough")
        .args(["--json", "broadcast", "send", "1"]);
    cmd.assert().success();

    // Feed a delivered event back for the resend_email_id the stub assigned.
    let stub_response = r#"{"version":"1","status":"success","data":{"object":"list","has_more":false,"data":[{"id":"em_stub_1","to":["alice@example.com"],"last_event":"delivered","created_at":"2026-04-08T12:00:00Z","subject":"Hi"}]}}"#;

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .env("MLC_STUB_EMAIL_LIST_JSON", stub_response)
        .args(["--json", "event", "poll"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["processed"].as_i64().unwrap(), 1);

    // Verify report show reports the delivered count + sane CTR (zero clicks
    // means CTR = 0, but the field MUST be present and numeric).
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args(["--json", "report", "show", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["status"], "success");
    let summary = &v["data"]["summary"];
    assert_eq!(summary["broadcast_id"].as_i64().unwrap(), 1);
    assert_eq!(summary["broadcast_name"], "phase6-report");
    assert_eq!(summary["delivered_count"].as_i64().unwrap(), 1);
    assert_eq!(summary["bounced_count"].as_i64().unwrap(), 0);
    assert_eq!(summary["clicked_count"].as_i64().unwrap(), 0);
    assert!(summary["ctr"].is_number());
    assert_eq!(summary["ctr"].as_f64().unwrap(), 0.0);
    assert!(summary["bounce_rate"].is_number());
    assert_eq!(summary["bounce_rate"].as_f64().unwrap(), 0.0);
    assert!(summary["complaint_rate"].is_number());
    assert!(summary["open_rate"].is_number());
}

#[test]
fn report_show_for_nonexistent_broadcast_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "report", "show", "999"]);
    let assert = cmd.assert().failure().code(3);
    // Errors are written to stderr, not stdout.
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let v: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(v["status"], "error");
    assert_eq!(v["error"]["code"], "broadcast_not_found");
}

#[test]
fn report_links_for_broadcast_with_no_clicks_returns_empty_array() {
    let (_tmp, config_path, db_path, cache_dir) = seed_broadcast_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args([
            "--json",
            "broadcast",
            "create",
            "--name",
            "no-clicks",
            "--template",
            "simple_ad",
            "--to",
            "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_CACHE_DIR", &cache_dir)
        .args(["--json", "report", "links", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["status"], "success");
    assert_eq!(v["data"]["links"].as_array().unwrap().len(), 0);
    assert_eq!(v["data"]["total_clicks"].as_i64().unwrap(), 0);
}

#[test]
fn report_deliverability_returns_zero_metrics_on_empty_db() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "report", "deliverability", "--days", "30"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["status"], "success");
    let report = &v["data"]["report"];
    assert_eq!(report["window_days"].as_i64().unwrap(), 30);
    assert_eq!(report["total_sent"].as_i64().unwrap(), 0);
    assert_eq!(report["bounce_rate"].as_f64().unwrap(), 0.0);
    assert_eq!(report["complaint_rate"].as_f64().unwrap(), 0.0);
    assert_eq!(report["verified_domains"].as_array().unwrap().len(), 0);
}

#[test]
fn report_engagement_returns_zero_on_empty_db() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "report", "engagement", "--days", "7"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["status"], "success");
    assert_eq!(v["data"]["target"], "all");
    assert_eq!(v["data"]["days"].as_i64().unwrap(), 7);
    assert_eq!(v["data"]["opens"].as_i64().unwrap(), 0);
    assert_eq!(v["data"]["clicks"].as_i64().unwrap(), 0);
    assert_eq!(v["data"]["engagement_score"].as_i64().unwrap(), 0);
}

#[test]
fn webhook_test_to_closed_port_fails() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "webhook",
            "test",
            "--to",
            "http://127.0.0.1:1", // guaranteed-closed port
            "--event",
            "delivered",
        ]);
    // curl will fail to connect to a closed port; we just assert non-success.
    cmd.assert().failure();
}

#[test]
fn webhook_test_unknown_event_type_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "webhook",
            "test",
            "--to",
            "http://127.0.0.1:1",
            "--event",
            "definitely-not-a-real-event",
        ]);
    cmd.assert().failure().code(3);
}
