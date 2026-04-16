use std::process::Command;

use assert_cmd::prelude::*;

fn help_command() -> Command {
    Command::cargo_bin("radroots").expect("binary")
}

#[test]
fn root_help_is_workflow_grouped() {
    let output = help_command()
        .arg("--help")
        .output()
        .expect("run root help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Start here"));
    assert!(stdout.contains("Sell from your farm"));
    assert!(stdout.contains("Buy from the market"));
    assert!(stdout.contains("Accounts and settings"));
    assert!(stdout.contains("Advanced and troubleshooting"));
    assert!(stdout.contains("setup"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("market"));
    assert!(stdout.contains("sell"));
    assert!(stdout.contains("Examples"));
}

#[test]
fn account_help_prefers_human_first_aliases() {
    let output = help_command()
        .args(["account", "--help"])
        .output()
        .expect("run account help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("create"));
    assert!(stdout.contains("view"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("select"));
    assert!(stdout.contains("Compatibility aliases: new, whoami, ls, use."));
}

#[test]
fn farm_help_mentions_human_first_subcommands() {
    let output = help_command()
        .args(["farm", "--help"])
        .output()
        .expect("run farm help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("init"));
    assert!(stdout.contains("set"));
    assert!(stdout.contains("check"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("publish"));
    assert!(stdout.contains(
        "Compatibility paths: `farm setup`, `farm status`, and `farm get` remain available."
    ));
}

#[test]
fn market_help_is_example_first() {
    let output = help_command()
        .args(["market", "--help"])
        .output()
        .expect("run market help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("update"));
    assert!(stdout.contains("search"));
    assert!(stdout.contains("view"));
    assert!(stdout.contains("radroots market search tomatoes"));
    assert!(
        stdout.contains(
            "Compatibility paths: `sync pull`, `find`, and `listing get` remain available."
        )
    );
}

#[test]
fn sell_help_mentions_listing_compatibility() {
    let output = help_command()
        .args(["sell", "--help"])
        .output()
        .expect("run sell help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("add"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("check"));
    assert!(stdout.contains("publish"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("pause"));
    assert!(stdout.contains("reprice"));
    assert!(stdout.contains("restock"));
    assert!(
        stdout.contains(
            "Compatibility path: the advanced `listing` command family remains available."
        )
    );
}

#[test]
fn order_help_prefers_create_view_and_list() {
    let output = help_command()
        .args(["order", "--help"])
        .output()
        .expect("run order help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("create"));
    assert!(stdout.contains("view"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("submit"));
    assert!(stdout.contains("watch"));
    assert!(stdout.contains("cancel"));
    assert!(stdout.contains("history"));
    assert!(stdout.contains("Compatibility aliases: new, get, ls."));
}
