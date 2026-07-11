use radroots_mesh::{
    RADROOTS_MESH_PREVIEW_DENIAL_MESSAGE, RadrootsMeshAdmissionInput, RadrootsMeshPayloadPolicy,
    RadrootsMeshPrivacyClass, RadrootsMeshScope,
};
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
        message: RADROOTS_MESH_PREVIEW_DENIAL_MESSAGE.to_owned(),
    }
}

pub fn policy_check(config: &RuntimeConfig) -> MeshPolicyCheckView {
    let policy = RadrootsMeshPayloadPolicy::preview_unavailable();
    let privacy_class = RadrootsMeshPrivacyClass::PublicEvent;
    let input = RadrootsMeshAdmissionInput::new(RadrootsMeshScope::Local, privacy_class, 1, 1);
    let decision = policy.evaluate(&input);
    let deny_reason = decision
        .deny_reason()
        .map(|reason| reason.label())
        .unwrap_or("none");

    MeshPolicyCheckView {
        state: "ready".to_owned(),
        source: MESH_SOURCE.to_owned(),
        scope: config.mesh.scope.as_str().to_owned(),
        policy: policy.policy_id().to_owned(),
        transport: "reticulum".to_owned(),
        usable_for_delivery: decision.usable_for_delivery(),
        decision: decision.label().to_owned(),
        deny_reason: deny_reason.to_owned(),
        privacy_class: privacy_class.label().to_owned(),
        payload_bytes: input.payload_bytes,
        frame_bytes: input.frame_bytes,
        max_payload_bytes: policy.max_payload_bytes,
        max_frame_bytes: policy.max_frame_bytes,
        compression: policy.compression.label().to_owned(),
        custom_scopes_enabled: policy.custom_scopes_enabled,
        message: decision.message().to_owned(),
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
