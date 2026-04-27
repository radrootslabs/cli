mod support;

use support::RadrootsCliSandbox;

#[test]
fn harness_runs_local_signer_status_with_json_envelope() {
    let sandbox = RadrootsCliSandbox::new();

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["schema_version"], "radroots.cli.output.v1");
    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["kind"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "local");
}

#[cfg(unix)]
#[test]
fn harness_runs_myc_signer_status_with_fake_executable() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = sandbox.write_fake_myc("myc-invalid-json", "printf 'not json\\n'");
    sandbox.write_app_config(&format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
        myc.display()
    ));

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
}
