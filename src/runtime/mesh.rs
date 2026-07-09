use serde_json::Value as JsonValue;
use toml::{Value, map::Map};

use crate::ops::OperationData;
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::transport::update_app_config_table;
use crate::view::runtime::{MeshPolicyCheckView, MeshScopeView, MeshStatusView};

const MESH_SOURCE: &str = "mesh config";

pub fn scope(config: &RuntimeConfig) -> MeshScopeView {
    scope_view(config.mesh.scope.as_str(), "ready")
}

pub fn set_scope(
    config: &RuntimeConfig,
    input: &OperationData,
) -> Result<MeshScopeView, RuntimeError> {
    let scope = input
        .get("scope")
        .and_then(JsonValue::as_str)
        .unwrap_or("disabled");
    match scope {
        "disabled" | "local_preview" => {}
        other => {
            return Err(RuntimeError::Config(format!(
                "mesh scope `{other}` is not supported"
            )));
        }
    }
    let mut mesh = Map::new();
    mesh.insert("scope".to_owned(), Value::String(scope.to_owned()));
    update_app_config_table(config, "mesh", Value::Table(mesh))?;
    Ok(scope_view(scope, "configured"))
}

pub fn status(config: &RuntimeConfig) -> MeshStatusView {
    let scope = config.mesh.scope.as_str();
    MeshStatusView {
        state: "ready".to_owned(),
        source: MESH_SOURCE.to_owned(),
        scope: scope.to_owned(),
        transport: "reticulum".to_owned(),
        configured: scope != "disabled",
        implementation: "preview_unavailable".to_owned(),
        usable_for_delivery: false,
        message: "Reticulum mesh preview is explicit and unavailable for real delivery".to_owned(),
    }
}

pub fn policy_check(config: &RuntimeConfig) -> MeshPolicyCheckView {
    MeshPolicyCheckView {
        state: "ready".to_owned(),
        source: MESH_SOURCE.to_owned(),
        scope: config.mesh.scope.as_str().to_owned(),
        policy: "reticulum_preview_delivery".to_owned(),
        transport: "reticulum".to_owned(),
        usable_for_delivery: false,
        decision: "reject_delivery_attempt".to_owned(),
        message: "Reticulum preview never falls back to Nostr and cannot deliver real events"
            .to_owned(),
    }
}

fn scope_view(scope: &str, state: &str) -> MeshScopeView {
    MeshScopeView {
        state: state.to_owned(),
        source: MESH_SOURCE.to_owned(),
        scope: scope.to_owned(),
        implementation: "preview_unavailable".to_owned(),
        message: "Mesh delivery is disabled unless a preview scope is explicitly configured"
            .to_owned(),
        actions: vec!["radroots mesh policy check".to_owned()],
    }
}
