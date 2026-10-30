//! What this server instance can and cannot do, recorded while it is built. Surfaced to the model
//! by `visma_init` (a tool that is missing is explained, not a mystery) and to the operator on
//! stderr.

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// Whose authority the live tools use unless a sign-in says otherwise.
pub const SERVICE_ACTS_AS: &str = "the integration's service credentials, not the signed-in user: every tenant it is configured for \
    is readable by whoever can call it, so do not assume the user's own Visma roles apply here";

#[derive(Debug, Clone, Default)]
pub struct ServerStatus {
    /// One line per capability that was left out, for the startup log.
    pub notes: Vec<String>,
    capabilities: BTreeMap<String, Value>,
    acts_as: Option<String>,
}

impl ServerStatus {
    pub fn enabled(&mut self, name: &str, details: Value) {
        let mut v = json!({"enabled": true});
        if let (Some(base), Value::Object(more)) = (v.as_object_mut(), details) {
            base.extend(more);
        }
        self.capabilities.insert(name.into(), v);
    }

    pub fn disabled(&mut self, name: &str, reason: impl Into<String>) {
        let reason = reason.into();
        self.notes.push(format!("{name} disabled: {reason}"));
        self.capabilities
            .insert(name.into(), json!({"enabled": false, "reason": reason}));
    }

    /// Say whose Visma authority the live tools use, when it is not the service credentials.
    pub fn set_acts_as(&mut self, text: impl Into<String>) {
        self.acts_as = Some(text.into());
    }

    pub fn acts_as(&self) -> &str {
        self.acts_as.as_deref().unwrap_or(SERVICE_ACTS_AS)
    }

    /// A warning that is not tied to one capability.
    pub fn note(&mut self, msg: impl Into<String>) {
        self.notes.push(msg.into());
    }

    pub fn capabilities_json(&self) -> Value {
        json!(self.capabilities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_enabled_and_disabled_capabilities_and_logs_the_reasons() {
        let mut s = ServerStatus::default();
        s.enabled("warehouse", json!({"project": "p"}));
        s.disabled("writes", "VSV2_MCP_ENABLE_WRITES is not set");
        s.note("heads up");
        let c = s.capabilities_json();
        assert_eq!(
            (
                c["warehouse"]["enabled"].as_bool(),
                c["warehouse"]["project"].as_str()
            ),
            (Some(true), Some("p"))
        );
        assert_eq!(c["writes"]["reason"], "VSV2_MCP_ENABLE_WRITES is not set");
        assert_eq!(
            s.notes,
            [
                "writes disabled: VSV2_MCP_ENABLE_WRITES is not set",
                "heads up"
            ]
        );
    }
}
