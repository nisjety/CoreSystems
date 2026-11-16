//! Run-action registry for the Space agent self-API (WS18-S1).
//!
//! The golden `apps/Model Plane/api/contracts/space-self-api-registry.json`
//! lists every run-bound agent action. This crate compiles a byte-identical
//! copy (`contracts/space-self-api-registry.json`; the repository sync check
//! pins the golden and the copy). The module owns the two derivations that
//! execution-core and Session Core must agree on, the payload digest and the
//! idempotency key. Control never sees arguments, so only Rust computes the
//! digest.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;

const REGISTRY_JSON: &str = include_str!("../contracts/space-self-api-registry.json");

/// Contract name the golden must carry.
pub const REGISTRY_CONTRACT: &str = "space-self-api-registry";
/// Schema version the golden must carry.
pub const REGISTRY_VERSION: u32 = 1;
/// Domain tag that opens every payload digest preimage.
pub const PAYLOAD_DIGEST_DOMAIN: &str = "space-self-api/v1";
/// The one legacy row that predates the registry. Its schema hash is pinned, not derived.
pub const LEGACY_ACTION_ID: &str = "tickets.create";

const ROLES: [&str; 4] = ["viewer", "editor", "manager", "owner"];
const FORBIDDEN_NAME_PARTS: [&str; 11] = [
    "member",
    "role",
    "delete",
    "credential",
    "grant",
    "policy",
    "thread_id",
    "space",
    "org",
    "user",
    "actor",
];

/// Why a golden copy was refused. Callers map it to a refusal and never to a
/// downgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The document is not one strict JSON object of the golden shape.
    Malformed(String),
    /// The header or a row breaks a rule the registry test also enforces.
    Invalid(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(f, "space self-api registry is malformed: {detail}"),
            Self::Invalid(detail) => write!(f, "space self-api registry is invalid: {detail}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// One golden row. `deny_unknown_fields` keeps this reader in step with the golden.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryRow {
    pub action_id: String,
    pub capability_id: String,
    pub owner_plane: String,
    pub owner_service: String,
    pub service_audience: String,
    pub decision_type: String,
    pub permission: String,
    pub min_role: String,
    pub risk: String,
    pub default_level: String,
    pub approval: String,
    pub autonomy_rung: String,
    pub reversible: bool,
    pub max_per_run: u32,
    pub reservation: bool,
    pub argument_order: Vec<String>,
    pub parameters_json: String,
    pub schema_sha256: String,
    pub hash_basis: String,
    pub zdr: String,
    pub ui_action_id: Option<String>,
    pub phase: u8,
}

/// The golden document after strict decoding.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub contract: String,
    pub version: u32,
    pub rows: Vec<RegistryRow>,
    pub digest_vectors: serde_json::Value,
}

impl Registry {
    /// The row the golden names for an action id.
    pub fn row(&self, action_id: &str) -> Option<&RegistryRow> {
        self.rows.iter().find(|row| row.action_id == action_id)
    }

    fn validate(&self) -> Result<(), RegistryError> {
        if self.contract != REGISTRY_CONTRACT
            || self.version != REGISTRY_VERSION
            || !self.digest_vectors.is_array()
        {
            return Err(RegistryError::Invalid("header".to_owned()));
        }
        let mut ids = BTreeSet::new();
        for row in &self.rows {
            if !ids.insert(row.action_id.as_str()) {
                return Err(RegistryError::Invalid(format!(
                    "{}: repeated action id",
                    row.action_id
                )));
            }
            row.validate()?;
        }
        Ok(())
    }
}

impl RegistryRow {
    fn validate(&self) -> Result<(), RegistryError> {
        let invalid = |detail: &str| RegistryError::Invalid(format!("{}: {detail}", self.action_id));
        if !action_id_is_valid(&self.action_id) {
            return Err(invalid("action id pattern"));
        }
        let tail = self
            .action_id
            .strip_prefix("space.")
            .unwrap_or(self.action_id.as_str());
        if forbidden_name(tail).is_some() {
            return Err(invalid("action id names a forbidden part"));
        }
        if self.capability_id.is_empty()
            || self.owner_plane.is_empty()
            || self.owner_service.is_empty()
            || self.service_audience.is_empty()
            || self.permission.is_empty()
            || self.risk.is_empty()
            || self.autonomy_rung.is_empty()
        {
            return Err(invalid("row is incomplete"));
        }
        if !matches!(self.decision_type.as_str(), "run_action" | "schedule_create") {
            return Err(invalid("unknown decision type"));
        }
        if !ROLES.contains(&self.min_role.as_str()) {
            return Err(invalid("unknown min role"));
        }
        if !matches!(self.default_level.as_str(), "allow" | "ask")
            || !matches!(self.approval.as_str(), "none" | "owner_approval")
            || self.zdr != "refuse"
        {
            return Err(invalid("invalid policy field"));
        }
        if self.phase > 3 || (self.phase > 0 && self.max_per_run == 0) {
            return Err(invalid("invalid phase or cap"));
        }
        // The owner reservation table admits tickets.create only.
        if self.reservation && self.action_id != LEGACY_ACTION_ID {
            return Err(invalid("reservation admits tickets.create only"));
        }
        if self
            .ui_action_id
            .as_deref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(invalid("empty ui action id"));
        }
        if !is_sha256_commitment(&self.schema_sha256) {
            return Err(invalid("schema hash shape"));
        }
        let basis_holds = match self.hash_basis.as_str() {
            "owner_contract" => self.action_id == LEGACY_ACTION_ID,
            "parameters_json" => {
                self.schema_sha256 == sha256_commitment(self.parameters_json.as_bytes())
            }
            _ => false,
        };
        if !basis_holds {
            return Err(invalid("hash basis does not hold"));
        }
        self.validate_parameters()
    }

    /// The closed argument schema a model sees: an object with
    /// `additionalProperties` false, one property per argument, only scalar or
    /// array types, and no `format` or `$ref` keyword (no URL or markup field).
    fn validate_parameters(&self) -> Result<(), RegistryError> {
        let invalid = |detail: &str| RegistryError::Invalid(format!("{}: {detail}", self.action_id));
        let schema: serde_json::Value = serde_json::from_str(&self.parameters_json)
            .map_err(|_| invalid("parameters_json is not JSON"))?;
        if schema.get("type").and_then(serde_json::Value::as_str) != Some("object")
            || schema
                .get("additionalProperties")
                .and_then(serde_json::Value::as_bool)
                != Some(false)
            || contains_unsafe_keyword(&schema)
        {
            return Err(invalid("parameters_json is not a closed object"));
        }
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| invalid("parameters_json has no properties"))?;
        if properties.len() != self.argument_order.len() {
            return Err(invalid("argument_order does not name each property"));
        }
        let mut seen = BTreeSet::new();
        for name in &self.argument_order {
            let property = properties
                .get(name)
                .ok_or_else(|| invalid("argument is not a property"))?;
            if !seen.insert(name.as_str()) || forbidden_name(name).is_some() {
                return Err(invalid("argument repeats or names a forbidden part"));
            }
            let kind = property
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if !matches!(kind, "string" | "boolean" | "integer" | "array") {
                return Err(invalid("argument has an unsupported type"));
            }
        }
        Ok(())
    }
}

fn action_id_is_valid(id: &str) -> bool {
    if id == LEGACY_ACTION_ID {
        return true;
    }
    let segments: Vec<&str> = id.split('.').collect();
    segments.len() == 3
        && segments[0] == "space"
        && segments[1..].iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        })
}

fn forbidden_name(name: &str) -> Option<&'static str> {
    FORBIDDEN_NAME_PARTS
        .iter()
        .copied()
        .find(|part| name.contains(*part))
}

fn contains_unsafe_keyword(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, child)| {
            key.as_str() == "format" || key.as_str() == "$ref" || contains_unsafe_keyword(child)
        }),
        serde_json::Value::Array(items) => items.iter().any(contains_unsafe_keyword),
        _ => false,
    }
}

fn is_sha256_commitment(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// `sha256:` followed by the lowercase hex digest of `bytes`.
pub fn sha256_commitment(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// Decodes and validates one golden document.
pub fn load(raw: &str) -> Result<Registry, RegistryError> {
    let registry: Registry = serde_json::from_str(raw)
        .map_err(|error| RegistryError::Malformed(error.to_string()))?;
    registry.validate()?;
    Ok(registry)
}

static REGISTRY: OnceLock<Result<Registry, RegistryError>> = OnceLock::new();

/// The validated golden compiled into this crate. A malformed copy is reported
/// on every call, so a caller that maps the error to a refusal fails closed.
pub fn registry() -> Result<&'static Registry, &'static RegistryError> {
    REGISTRY.get_or_init(|| load(REGISTRY_JSON)).as_ref()
}

/// Payload digest for one run-action call. `argument_pairs` must list the
/// arguments in the row's `argument_order`, with arrays already flattened to
/// `fields.len`, `fields.<i>.label` and `fields.<i>.value`. Every field is framed
/// as a u32 big-endian byte length and its UTF-8 bytes, so no value can forge a
/// boundary. No JSON is hashed, because serializers escape `&` and `<` differently.
pub fn payload_digest(
    action_id: &str,
    run_id: &str,
    org_id: &str,
    idempotency_key: &str,
    argument_pairs: &[(&str, &str)],
) -> Result<String, RegistryError> {
    let mut hasher = Sha256::new();
    hasher.update(PAYLOAD_DIGEST_DOMAIN.as_bytes());
    hasher.update([0u8]);
    for field in [action_id, run_id, org_id, idempotency_key] {
        frame(&mut hasher, field)?;
    }
    for (name, value) in argument_pairs {
        frame(&mut hasher, name)?;
        frame(&mut hasher, value)?;
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn frame(hasher: &mut Sha256, field: &str) -> Result<(), RegistryError> {
    let length = u32::try_from(field.len())
        .map_err(|_| RegistryError::Invalid("field exceeds the framing limit".to_owned()))?;
    hasher.update(length.to_be_bytes());
    hasher.update(field.as_bytes());
    Ok(())
}

/// `"{action_id}:" + hex(sha256(action_id 0x00 run_id 0x00 step_id))`.
pub fn idempotency_key(action_id: &str, run_id: &str, step_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(action_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(run_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(step_id.as_bytes());
    format!("{action_id}:{:x}", hasher.finalize())
}
