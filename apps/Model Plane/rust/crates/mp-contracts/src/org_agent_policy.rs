//! The org agent policy registry's closed key schema and its tighten-only check
//! (build plan WS25-S7, DEC-14).
//!
//! `SETTINGS` is the schema as data: every key the registry accepts, its kind, its
//! values from the strictest to the loosest, the value a key takes when no row has been
//! written, the value enforcement falls back to when a stored value cannot be read, and
//! whether the key is enforced yet or reserved for a later stream. A reserved key takes
//! only its default; anything else is refused as `not_enforced` rather than stored and
//! silently ignored. A key that is enforced may still reserve single values
//! (`reserved_values`), refused the same way. capability-core validates writes against this table and
//! `api/contracts/org-agent-policy-schema.json` pins it, so a key added on one side and
//! not the other fails a test.
//!
//! A write is tighten-only against a `Ceiling`, the loosest value the platform allows for
//! each key. A value looser than its ceiling is refused.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

/// The settings document: every key of `SETTINGS`, with the value in force.
pub type Settings = BTreeMap<String, Value>;

/// The loosest value the platform allows for a key, by key. A key absent from the map
/// is bounded only by its schema.
pub type Ceiling = BTreeMap<&'static str, &'static str>;

/// Most ids one allow-list may name.
pub const MAX_ALLOW_LIST_IDS: usize = 64;

const MAX_ID_LEN: usize = 200;

/// How a key's value is shaped on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A string from `strictness_order`.
    Enum,
    /// A JSON boolean; `true` is the stricter value.
    Bool,
    /// `{"mode": <from strictness_order>, "ids": [...]}`. Only `approved_only` names ids.
    AllowList,
    /// An [`Kind::AllowList`] whose ids are canonical `https` origins (see [`is_https_origin`]).
    OriginAllowList,
    /// A JSON whole number whose decimal form is in `strictness_order`, smallest (strictest)
    /// first, so a count is tighten-only by the same rank as every other kind.
    Integer,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enum => "enum",
            Self::Bool => "bool",
            Self::AllowList => "allow_list",
            Self::OriginAllowList => "origin_allow_list",
            Self::Integer => "integer",
        }
    }
}

/// The org's wide-research item cap (Wave 3 EP-2 S8, D5): the most items one wide-research run
/// may fan out. `0` turns wide research off for the org.
pub const RESEARCH_MAX_FAN_OUT_ITEMS: &str = "research.max_fan_out_items";

/// The value of [`RESEARCH_MAX_FAN_OUT_ITEMS`] an org with no row has: execution-core's default
/// child ceiling, so an org that never sets the key fans out exactly as wide as a run did before.
pub const DEFAULT_MAX_FAN_OUT_ITEMS: u32 = 8;

/// The loosest value [`RESEARCH_MAX_FAN_OUT_ITEMS`] accepts: execution-core's fan-out ceiling.
pub const MAX_FAN_OUT_ITEMS: u32 = 16;

/// Reads a stored [`RESEARCH_MAX_FAN_OUT_ITEMS`] value: a whole number from 0 to
/// [`MAX_FAN_OUT_ITEMS`]. Anything else is `None`, and the reader takes the fail-closed `0`.
#[must_use]
pub fn fan_out_items_of(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|count| u32::try_from(count).ok())
        .filter(|count| *count <= MAX_FAN_OUT_ITEMS)
}

/// The origins an MCP App may reach over the network (Wave 4 W4-D M1a): an allow-list of canonical
/// `https` origins. Reserved until the switch that serves the app's CSP is on (`MCP_APPS_NETWORK_ENABLED`,
/// model-gateway), so only the default `none` is accepted and every app keeps the no-network frame it has.
pub const MCP_APPS_NETWORK_ORIGINS: &str = "mcp_apps.network_origins";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Enforced,
    Reserved,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforced => "enforced",
            Self::Reserved => "reserved",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingDef {
    pub key: &'static str,
    pub kind: Kind,
    /// The values (or allow-list modes) from the strictest to the loosest. For a bool,
    /// `"true"` then `"false"`.
    pub strictness_order: &'static [&'static str],
    pub default: &'static str,
    pub fail_closed_value: &'static str,
    pub status: Status,
    /// Tokens of `strictness_order` this build does not enforce yet. A write of one is refused
    /// `not_enforced` while the key itself is enforced, so a value is never stored and then
    /// silently ignored. Empty for a key whose every token is enforced.
    pub reserved_values: &'static [&'static str],
}

pub const SETTINGS: &[SettingDef] = &[
    SettingDef {
        key: "mode_cap",
        kind: Kind::Enum,
        strictness_order: &["lockdown", "read_only", "ask_first", "auto"],
        default: "auto",
        fail_closed_value: "lockdown",
        status: Status::Enforced,
        reserved_values: &[],
    },
    SettingDef {
        key: "rung_cap",
        kind: Kind::Enum,
        strictness_order: &["read_only", "workspace_write", "danger_full_access"],
        default: "danger_full_access",
        fail_closed_value: "read_only",
        status: Status::Enforced,
        reserved_values: &[],
    },
    SettingDef {
        key: "lockdown",
        kind: Kind::Bool,
        strictness_order: &["true", "false"],
        default: "false",
        fail_closed_value: "true",
        status: Status::Enforced,
        reserved_values: &[],
    },
    SettingDef {
        key: "ext.mcp_servers",
        kind: Kind::AllowList,
        strictness_order: &["none", "approved_only", "any"],
        default: "any",
        fail_closed_value: "none",
        status: Status::Enforced,
        reserved_values: &[],
    },
    SettingDef {
        key: "ext.skills",
        kind: Kind::AllowList,
        strictness_order: &["none", "approved_only", "any"],
        default: "any",
        fail_closed_value: "none",
        status: Status::Reserved,
        reserved_values: &[],
    },
    SettingDef {
        key: "ext.plugins",
        kind: Kind::AllowList,
        strictness_order: &["none", "approved_only", "any"],
        default: "any",
        fail_closed_value: "none",
        status: Status::Reserved,
        reserved_values: &[],
    },
    SettingDef {
        key: "mcp_tool_approval",
        kind: Kind::Enum,
        strictness_order: &["approved_only", "trust_annotations"],
        default: "trust_annotations",
        fail_closed_value: "approved_only",
        status: Status::Enforced,
        reserved_values: &[],
    },
    SettingDef {
        key: "user_hooks",
        kind: Kind::Enum,
        strictness_order: &["denied", "allowed"],
        default: "allowed",
        fail_closed_value: "denied",
        status: Status::Reserved,
        reserved_values: &[],
    },
    // WS08-S8 / DEC-37: the per-org opt-in for durable reference uploads. Reserved until a
    // consumer enforces it (the WS25-S9 policy client, behind a CHAT_REFERENCE_UPLOADS_ENABLED
    // kill switch), so any value but the default is refused as not_enforced.
    SettingDef {
        key: "reference_uploads",
        kind: Kind::Enum,
        strictness_order: &["off", "on"],
        default: "off",
        fail_closed_value: "off",
        status: Status::Reserved,
        reserved_values: &[],
    },
    // Wave 3 EP-2 S8 (D5): the wide-research item cap. Enforced by the gateway when a run starts
    // (behind WIDE_RESEARCH_ENTRY_ENABLED), which sends it to execution-core as the run's
    // `WideResearchRequest.item_cap`; `0` turns wide research off.
    SettingDef {
        key: RESEARCH_MAX_FAN_OUT_ITEMS,
        kind: Kind::Integer,
        strictness_order: &[
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
            "16",
        ],
        default: "8",
        fail_closed_value: "0",
        status: Status::Enforced,
        reserved_values: &[],
    },
    // Wave 3 EP-9 W2: the org's web switch. `off` withholds public web access from every agent path
    // (the inline loop and the Do loop); `cached_only` is reserved until Quarry can answer from its
    // cache alone, so a write of it is refused rather than stored. Fail-closed is `off`.
    SettingDef {
        key: "web.mode",
        kind: Kind::Enum,
        strictness_order: &["off", "cached_only", "live"],
        default: "live",
        fail_closed_value: "off",
        status: Status::Enforced,
        reserved_values: &["cached_only"],
    },
    // Wave 3 EP-9 W2: whether a paid search provider may serve the org's agents. Fail-closed is `off`.
    SettingDef {
        key: "web.paid_providers",
        kind: Kind::Enum,
        strictness_order: &["off", "on"],
        default: "on",
        fail_closed_value: "off",
        status: Status::Enforced,
        reserved_values: &[],
    },
    // Wave 4 W4-D M1a: the origins an MCP App may reach. `approved_only` names origins; it is reserved
    // until MCP_APPS_NETWORK_ENABLED serves them, so a write of it is refused `not_enforced`. Fail-closed is `none`.
    SettingDef {
        key: MCP_APPS_NETWORK_ORIGINS,
        kind: Kind::OriginAllowList,
        strictness_order: &["none", "approved_only"],
        default: "none",
        fail_closed_value: "none",
        status: Status::Reserved,
        reserved_values: &[],
    },
];

/// Why a document is refused. The codes are the closed vocabulary capability-core
/// returns; the golden lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    UnknownKey,
    InvalidValue,
    NotEnforced,
    ExceedsPlatformCeiling,
}

impl Refusal {
    pub const ALL: [Self; 4] = [
        Self::UnknownKey,
        Self::InvalidValue,
        Self::NotEnforced,
        Self::ExceedsPlatformCeiling,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::UnknownKey => "unknown_key",
            Self::InvalidValue => "invalid_value",
            Self::NotEnforced => "not_enforced",
            Self::ExceedsPlatformCeiling => "exceeds_platform_ceiling",
        }
    }
}

/// The ceiling that bounds nothing: every key at its loosest schema value.
pub fn loosest_ceiling() -> Ceiling {
    SETTINGS
        .iter()
        .filter_map(|def| {
            def.strictness_order
                .last()
                .map(|loosest| (def.key, *loosest))
        })
        .collect()
}

/// The document a platform with no row for an org answers with.
pub fn defaults() -> Settings {
    SETTINGS
        .iter()
        .map(|def| (def.key.to_owned(), value_of(def, def.default)))
        .collect()
}

/// Checks a written document and returns the complete document in force: a missing key
/// takes its default. Keys are checked in schema order after the unknown ones, so the
/// same input always yields the same refusal.
///
/// # Errors
///
/// The first refusal found, in that order.
pub fn validate(input: &Map<String, Value>, ceiling: &Ceiling) -> Result<Settings, Refusal> {
    if input
        .keys()
        .any(|key| !SETTINGS.iter().any(|def| def.key == key))
    {
        return Err(Refusal::UnknownKey);
    }
    let mut settings = Settings::new();
    for def in SETTINGS {
        let (token, value) = match input.get(def.key) {
            Some(written) => parse(def, written)?,
            None => (def.default.to_owned(), value_of(def, def.default)),
        };
        if def.status == Status::Reserved && token != def.default {
            return Err(Refusal::NotEnforced);
        }
        if def.reserved_values.contains(&token.as_str()) {
            return Err(Refusal::NotEnforced);
        }
        let loosest = def.strictness_order.last().copied().unwrap_or(def.default);
        let bound = ceiling.get(def.key).copied().unwrap_or(loosest);
        if rank(def, &token) > rank(def, bound) {
            return Err(Refusal::ExceedsPlatformCeiling);
        }
        settings.insert(def.key.to_owned(), value);
    }
    Ok(settings)
}

/// Reads one written value: its token in `strictness_order` and its canonical form.
fn parse(def: &SettingDef, written: &Value) -> Result<(String, Value), Refusal> {
    match def.kind {
        Kind::Enum => {
            let token = written.as_str().ok_or(Refusal::InvalidValue)?;
            if !def.strictness_order.contains(&token) {
                return Err(Refusal::InvalidValue);
            }
            Ok((token.to_owned(), Value::String(token.to_owned())))
        }
        Kind::Bool => {
            let flag = written.as_bool().ok_or(Refusal::InvalidValue)?;
            let token = if flag { "true" } else { "false" };
            Ok((token.to_owned(), Value::Bool(flag)))
        }
        Kind::AllowList | Kind::OriginAllowList => parse_allow_list(def, written),
        Kind::Integer => {
            // `as_u64` refuses a fraction, a negative and a quoted number, so `8.5`, `-1` and `"8"`
            // are invalid rather than rounded or read as text.
            let token = written
                .as_u64()
                .map(|count| count.to_string())
                .ok_or(Refusal::InvalidValue)?;
            if !def.strictness_order.contains(&token.as_str()) {
                return Err(Refusal::InvalidValue);
            }
            let value = value_of(def, &token);
            Ok((token, value))
        }
    }
}

fn parse_allow_list(def: &SettingDef, written: &Value) -> Result<(String, Value), Refusal> {
    let object = written.as_object().ok_or(Refusal::InvalidValue)?;
    if object.len() != 2 || !object.contains_key("mode") || !object.contains_key("ids") {
        return Err(Refusal::InvalidValue);
    }
    let mode = object["mode"].as_str().ok_or(Refusal::InvalidValue)?;
    if !def.strictness_order.contains(&mode) {
        return Err(Refusal::InvalidValue);
    }
    let raw_ids = object["ids"].as_array().ok_or(Refusal::InvalidValue)?;
    if !raw_ids.is_empty() && mode != "approved_only" {
        return Err(Refusal::InvalidValue);
    }
    let mut ids = BTreeSet::new();
    for raw in raw_ids {
        let id = raw.as_str().ok_or(Refusal::InvalidValue)?;
        let well_formed = match def.kind {
            Kind::OriginAllowList => is_https_origin(id),
            _ => is_identifier(id),
        };
        if !well_formed {
            return Err(Refusal::InvalidValue);
        }
        ids.insert(id.to_owned());
    }
    if ids.len() > MAX_ALLOW_LIST_IDS {
        return Err(Refusal::InvalidValue);
    }
    let ids: Vec<Value> = ids.into_iter().map(Value::String).collect();
    let mut canonical = Map::new();
    canonical.insert("mode".to_owned(), Value::String(mode.to_owned()));
    canonical.insert("ids".to_owned(), Value::Array(ids));
    Ok((mode.to_owned(), Value::Object(canonical)))
}

/// The canonical value of a token, for a token already known to be in the schema.
fn value_of(def: &SettingDef, token: &str) -> Value {
    match def.kind {
        Kind::Enum => Value::String(token.to_owned()),
        Kind::Bool => Value::Bool(token == "true"),
        Kind::AllowList | Kind::OriginAllowList => {
            let mut canonical = Map::new();
            canonical.insert("mode".to_owned(), Value::String(token.to_owned()));
            canonical.insert("ids".to_owned(), Value::Array(Vec::new()));
            Value::Object(canonical)
        }
        Kind::Integer => token
            .parse::<u64>()
            .map_or(Value::Null, |count| Value::Number(count.into())),
    }
}

/// Position in `strictness_order`: lower is stricter. A token outside the order ranks
/// loosest of all, so it can never pass a ceiling.
fn rank(def: &SettingDef, token: &str) -> usize {
    def.strictness_order
        .iter()
        .position(|candidate| *candidate == token)
        .unwrap_or(usize::MAX)
}

/// The shape of an MCP server id: the same rule capability-core applies to a server's
/// own identifier, so an allow-list can name only what the registry can hold.
fn is_identifier(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= MAX_ID_LEN
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// Whether `value` is one canonical `https` origin: `https://` and a lowercase host of dot-separated labels, with an
/// optional port from 1 to 65535 written without a sign or a leading zero. There is no path, query, fragment, userinfo,
/// wildcard or upper case, so an origin written into a CSP source list grants exactly the origin it names.
#[must_use]
pub fn is_https_origin(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (rest, None),
    };
    value.len() <= MAX_ID_LEN
        && !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(is_dns_label)
        && port.is_none_or(is_port)
}

/// One DNS label: lowercase letters, digits and inner hyphens, 1 to 63 bytes.
fn is_dns_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// A port in canonical decimal: digits only, no leading zero, and at most 65535.
fn is_port(port: &str) -> bool {
    !port.is_empty()
        && port.len() <= 5
        && !port.starts_with('0')
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(value: &Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn org_agent_policy_defaults_are_accepted_under_the_loosest_ceiling() {
        let accepted = validate(&Map::new(), &loosest_ceiling()).expect("defaults are valid");
        assert_eq!(accepted, defaults());
        assert_eq!(accepted.len(), SETTINGS.len());
    }

    #[test]
    fn org_agent_policy_unknown_key_is_refused_before_any_other_check() {
        let refused = validate(
            &doc(&json!({"mode_cap": "bogus", "nope": 1})),
            &loosest_ceiling(),
        );
        assert_eq!(refused, Err(Refusal::UnknownKey));
    }

    #[test]
    fn org_agent_policy_enum_outside_its_order_is_invalid() {
        let refused = validate(
            &doc(&json!({"mode_cap": "full_access"})),
            &loosest_ceiling(),
        );
        assert_eq!(refused, Err(Refusal::InvalidValue));
    }

    #[test]
    fn org_agent_policy_reserved_key_takes_only_its_default() {
        let plugins_none = json!({"mode": "none", "ids": []});
        assert_eq!(
            validate(
                &doc(&json!({"ext.plugins": plugins_none})),
                &loosest_ceiling()
            ),
            Err(Refusal::NotEnforced)
        );
        let plugins_default = json!({"mode": "any", "ids": []});
        assert!(validate(
            &doc(&json!({"ext.plugins": plugins_default})),
            &loosest_ceiling()
        )
        .is_ok());
    }

    #[test]
    fn org_agent_policy_reference_uploads_takes_only_off_until_it_is_enforced() {
        let settings = defaults();
        assert_eq!(settings["reference_uploads"], json!("off"));
        assert_eq!(
            validate(
                &doc(&json!({"reference_uploads": "on"})),
                &loosest_ceiling()
            ),
            Err(Refusal::NotEnforced)
        );
        assert!(validate(
            &doc(&json!({"reference_uploads": "off"})),
            &loosest_ceiling()
        )
        .is_ok());
        assert_eq!(
            validate(
                &doc(&json!({"reference_uploads": true})),
                &loosest_ceiling()
            ),
            Err(Refusal::InvalidValue)
        );
    }

    #[test]
    fn org_agent_policy_write_looser_than_the_ceiling_is_refused() {
        let mut ceiling = loosest_ceiling();
        ceiling.insert("mode_cap", "ask_first");
        assert_eq!(
            validate(&doc(&json!({"mode_cap": "auto"})), &ceiling),
            Err(Refusal::ExceedsPlatformCeiling)
        );
        assert!(validate(&doc(&json!({"mode_cap": "read_only"})), &ceiling).is_ok());
    }

    #[test]
    fn org_agent_policy_lockdown_true_is_the_stricter_value() {
        let mut ceiling = loosest_ceiling();
        ceiling.insert("lockdown", "true");
        assert_eq!(
            validate(&doc(&json!({"lockdown": false})), &ceiling),
            Err(Refusal::ExceedsPlatformCeiling)
        );
        assert!(validate(&doc(&json!({"lockdown": true})), &ceiling).is_ok());
    }

    #[test]
    fn org_agent_policy_allow_list_names_ids_only_under_approved_only() {
        let names_under_any = json!({"mode": "any", "ids": ["srv.one"]});
        assert_eq!(
            validate(
                &doc(&json!({"ext.mcp_servers": names_under_any})),
                &loosest_ceiling()
            ),
            Err(Refusal::InvalidValue)
        );
        let approved = json!({"mode": "approved_only", "ids": ["srv.one"]});
        assert!(validate(
            &doc(&json!({"ext.mcp_servers": approved})),
            &loosest_ceiling()
        )
        .is_ok());
    }

    #[test]
    fn org_agent_policy_allow_list_ids_are_sorted_deduplicated_and_shaped() {
        let written = json!({"mode": "approved_only", "ids": ["zeta", "alpha", "alpha"]});
        let settings = validate(
            &doc(&json!({"ext.mcp_servers": written})),
            &loosest_ceiling(),
        )
        .expect("valid ids");
        assert_eq!(
            settings["ext.mcp_servers"],
            json!({"mode": "approved_only", "ids": ["alpha", "zeta"]})
        );
        let spaced = json!({"mode": "approved_only", "ids": ["has space"]});
        assert_eq!(
            validate(
                &doc(&json!({"ext.mcp_servers": spaced})),
                &loosest_ceiling()
            ),
            Err(Refusal::InvalidValue)
        );
    }

    #[test]
    fn org_agent_policy_fan_out_items_defaults_to_eight_and_is_a_number() {
        let settings = defaults();
        assert_eq!(settings[RESEARCH_MAX_FAN_OUT_ITEMS], json!(8));
        assert_eq!(
            fan_out_items_of(&settings[RESEARCH_MAX_FAN_OUT_ITEMS]),
            Some(DEFAULT_MAX_FAN_OUT_ITEMS)
        );
        let accepted = validate(
            &doc(&json!({"research.max_fan_out_items": 5})),
            &loosest_ceiling(),
        )
        .expect("5 is inside the order");
        assert_eq!(accepted[RESEARCH_MAX_FAN_OUT_ITEMS], json!(5));
    }

    #[test]
    fn org_agent_policy_fan_out_items_refuses_text_fractions_and_values_past_sixteen() {
        for written in [json!("8"), json!(8.5), json!(-1), json!(17), json!(true)] {
            assert_eq!(
                validate(
                    &doc(&json!({"research.max_fan_out_items": written.clone()})),
                    &loosest_ceiling()
                ),
                Err(Refusal::InvalidValue),
                "{written}"
            );
        }
        assert_eq!(fan_out_items_of(&json!(17)), None);
        assert_eq!(fan_out_items_of(&json!("4")), None);
    }

    #[test]
    fn org_agent_policy_fan_out_items_is_tighten_only_and_zero_turns_it_off() {
        let mut ceiling = loosest_ceiling();
        assert_eq!(ceiling[RESEARCH_MAX_FAN_OUT_ITEMS], "16");
        ceiling.insert(RESEARCH_MAX_FAN_OUT_ITEMS, "4");
        assert_eq!(
            validate(&doc(&json!({"research.max_fan_out_items": 10})), &ceiling),
            Err(Refusal::ExceedsPlatformCeiling)
        );
        let off = validate(&doc(&json!({"research.max_fan_out_items": 0})), &ceiling)
            .expect("0 is the strictest value");
        assert_eq!(fan_out_items_of(&off[RESEARCH_MAX_FAN_OUT_ITEMS]), Some(0));
        let def = SETTINGS
            .iter()
            .find(|def| def.key == RESEARCH_MAX_FAN_OUT_ITEMS)
            .expect("the key is in the schema");
        assert_eq!(def.fail_closed_value, "0");
        assert_eq!(def.status, Status::Enforced);
    }

    #[test]
    fn org_agent_policy_web_mode_defaults_to_live_and_refuses_cached_only() {
        let settings = defaults();
        assert_eq!(settings["web.mode"], json!("live"));
        assert_eq!(settings["web.paid_providers"], json!("on"));
        assert_eq!(
            validate(&doc(&json!({"web.mode": "cached_only"})), &loosest_ceiling()),
            Err(Refusal::NotEnforced),
            "cached_only is reserved until Quarry ships it"
        );
        for mode in ["off", "live"] {
            assert!(
                validate(&doc(&json!({"web.mode": mode})), &loosest_ceiling()).is_ok(),
                "{mode} is enforced"
            );
        }
        assert_eq!(
            validate(&doc(&json!({"web.mode": "full"})), &loosest_ceiling()),
            Err(Refusal::InvalidValue)
        );
    }

    #[test]
    fn org_agent_policy_web_mode_off_is_stricter_than_live_under_a_ceiling() {
        let mut ceiling = loosest_ceiling();
        ceiling.insert("web.mode", "off");
        assert_eq!(
            validate(&doc(&json!({"web.mode": "live"})), &ceiling),
            Err(Refusal::ExceedsPlatformCeiling)
        );
        assert!(validate(&doc(&json!({"web.mode": "off"})), &ceiling).is_ok());
    }

    #[test]
    fn org_agent_policy_web_paid_providers_takes_off_or_on_and_nothing_else() {
        for token in ["off", "on"] {
            assert!(
                validate(
                    &doc(&json!({"web.paid_providers": token})),
                    &loosest_ceiling()
                )
                .is_ok(),
                "{token}"
            );
        }
        assert_eq!(
            validate(
                &doc(&json!({"web.paid_providers": true})),
                &loosest_ceiling()
            ),
            Err(Refusal::InvalidValue)
        );
    }

    #[test]
    fn org_agent_policy_mcp_app_origins_default_to_none_and_are_reserved() {
        assert_eq!(
            defaults()[MCP_APPS_NETWORK_ORIGINS],
            json!({"mode": "none", "ids": []})
        );
        assert_eq!(
            validate(
                &doc(&json!({"mcp_apps.network_origins": {"mode": "none", "ids": []}})),
                &loosest_ceiling()
            ),
            Ok(defaults())
        );
        assert_eq!(
            validate(
                &doc(
                    &json!({"mcp_apps.network_origins": {"mode": "approved_only", "ids": ["https://api.example.test"]}})
                ),
                &loosest_ceiling()
            ),
            Err(Refusal::NotEnforced),
            "reserved until MCP_APPS_NETWORK_ENABLED serves origins"
        );
    }

    #[test]
    fn org_agent_policy_mcp_app_origins_refuse_malformed_origins_before_the_reserved_check() {
        for ids in [
            json!(["http://api.example.test"]),
            json!(["https://api.example.test/app"]),
            json!(["https://*.example.test"]),
            json!(["https://API.example.test"]),
            json!(["https://api.example.test:0443"]),
            json!(["https://api.example.test:+443"]),
            json!(["https://api.example.test:65536"]),
            json!(["https://api.example.test; script-src *"]),
            json!(["https://"]),
            json!(["data:text/html,x"]),
            json!([7]),
        ] {
            assert_eq!(
                validate(
                    &doc(
                        &json!({"mcp_apps.network_origins": {"mode": "approved_only", "ids": ids.clone()}})
                    ),
                    &loosest_ceiling()
                ),
                Err(Refusal::InvalidValue),
                "{ids}"
            );
        }
        assert_eq!(
            validate(
                &doc(
                    &json!({"mcp_apps.network_origins": {"mode": "none", "ids": ["https://api.example.test"]}})
                ),
                &loosest_ceiling()
            ),
            Err(Refusal::InvalidValue),
            "ids are named only under approved_only"
        );
    }

    #[test]
    fn is_https_origin_accepts_only_the_canonical_form() {
        for origin in [
            "https://api.example.test",
            "https://api.example.test:8443",
            "https://a.b-c.d1.test:1",
            "https://xn--bcher-kva.example",
            "https://localhost:65535",
        ] {
            assert!(is_https_origin(origin), "{origin}");
        }
        for origin in [
            "",
            "https://",
            "http://api.example.test",
            "HTTPS://api.example.test",
            "https://API.example.test",
            "https://api.example.test/",
            "https://api.example.test/path",
            "https://api.example.test?q=1",
            "https://api.example.test#f",
            "https://user@api.example.test",
            "https://api.example.test:",
            "https://api.example.test:0",
            "https://api.example.test:08",
            "https://api.example.test:+443",
            "https://api.example.test:65536",
            "https://*.example.test",
            "https://api..example.test",
            "https://-api.example.test",
            "https://api-.example.test",
            "https://[::1]:443",
            "https://api.example.test.",
            "https://api.example.test a",
            "https://api.example.test'",
            "https://api.example.test;",
            "https://api.example.test,https://evil.test",
        ] {
            assert!(!is_https_origin(origin), "{origin}");
        }
        let long_label = format!("https://{}.test", "a".repeat(64));
        assert!(!is_https_origin(&long_label));
    }

    #[test]
    fn org_agent_policy_refusal_codes_are_the_closed_vocabulary() {
        let codes: Vec<&str> = Refusal::ALL.iter().map(|refusal| refusal.code()).collect();
        assert_eq!(
            codes,
            [
                "unknown_key",
                "invalid_value",
                "not_enforced",
                "exceeds_platform_ceiling"
            ]
        );
    }
}
