#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use radroots_cli::{
    HealthCommand, ProfileCommand, TargetCliArgs, TargetCommand, TargetOutputFormat,
};
use serde_json::{Value, json};

const OUTPUT_SCHEMA: &str = "radroots.cli.output.v1";

#[tokio::main]
async fn main() -> ExitCode {
    let args = TargetCliArgs::parse();
    let operation_id = args.command.operation_id();
    let format = args.format.unwrap_or(TargetOutputFormat::Terminal);

    let result = execute(&args.command).await;
    let (status, data, error, exit_code) = match result {
        Ok(data) => ("ok", data, Value::Null, ExitCode::SUCCESS),
        Err(message) => (
            "error",
            Value::Null,
            json!({
                "code": "unsupported_operation",
                "message": message,
                "recovery": "use `radroots health inspect` to inspect the release-v1 host surface"
            }),
            ExitCode::from(2),
        ),
    };

    let envelope = json!({
        "schema": OUTPUT_SCHEMA,
        "operation_id": operation_id,
        "status": status,
        "data": data,
        "error": error,
    });
    render(format, operation_id, status, &envelope);
    exit_code
}

async fn execute(command: &TargetCommand) -> Result<Value, String> {
    match command {
        TargetCommand::Profile(args) if matches!(args.command, ProfileCommand::Inspect) => {
            inspect_local_profile().await
        }
        TargetCommand::Health(args) if matches!(args.command, HealthCommand::Inspect) => {
            inspect_health().await
        }
        _ => Err(format!(
            "`{}` is not available in the crates-release-v1 CLI host",
            command.operation_id()
        )),
    }
}

async fn inspect_local_profile() -> Result<Value, String> {
    let client = radroots::client::memory()
        .build()
        .map_err(|error| format!("failed to construct the local SDK client: {error}"))?;
    let profile = radroots::client::local_only();
    client
        .close()
        .await
        .map_err(|error| format!("failed to close the local SDK client: {error}"))?;

    Ok(json!({
        "profile": "local_only",
        "network_enabled": !profile.is_local_only(),
        "storage": "memory",
        "artifact_graph": "registry"
    }))
}

async fn inspect_health() -> Result<Value, String> {
    let profile = inspect_local_profile().await?;
    Ok(json!({
        "ready": true,
        "release": "crates-v1",
        "profile": profile
    }))
}

fn render(format: TargetOutputFormat, operation_id: &str, status: &str, envelope: &Value) {
    match format {
        TargetOutputFormat::Json | TargetOutputFormat::Ndjson => println!("{envelope}"),
        TargetOutputFormat::Terminal => {
            if status == "ok" {
                println!("{operation_id}: ready");
            } else {
                let message = envelope["error"]["message"]
                    .as_str()
                    .unwrap_or("operation failed");
                eprintln!("{operation_id}: {message}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_operations_fail_closed() {
        let args = TargetCliArgs::try_parse_from(["radroots", "account", "list"])
            .expect("valid resource command");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let error = runtime
            .block_on(execute(&args.command))
            .expect_err("unsupported operation");
        assert!(error.contains("account.list"));
    }
}
