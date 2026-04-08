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
