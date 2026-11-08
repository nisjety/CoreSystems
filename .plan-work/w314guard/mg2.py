import sys
sys.path.insert(0, r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\w314guard')
from edit import edit

edit('src/org_agent_policy.rs', [
("""    let read = match def.kind {
        Kind::Enum => value.as_str(),
        Kind::Bool => value
            .as_bool()
            .map(|flag| if flag { "true" } else { "false" }),
        Kind::AllowList => value.get("mode").and_then(Value::as_str),
    };
""", """    // An integer key's token is its decimal form, held here so the match can lend it out.
    let count;
    let read = match def.kind {
        Kind::Enum => value.as_str(),
        Kind::Bool => value
            .as_bool()
            .map(|flag| if flag { "true" } else { "false" }),
        Kind::AllowList => value.get("mode").and_then(Value::as_str),
        Kind::Integer => {
            count = value.as_u64().map(|n| n.to_string());
            count.as_deref()
        }
    };
""", 1),
("""    pub restrictive: bool,
}
""", """    pub restrictive: bool,
    /// The most items one wide-research run may fan out (`research.max_fan_out_items`, EP-2 S8). A value this build
    /// cannot read is 0: wide research off for the org.
    pub max_fan_out_items: u32,
}
""", 1),
("""            restrictive: SETTINGS
                .iter()
                .any(|def| token_of(def, settings) != def.default),
        }
""", """            restrictive: SETTINGS
                .iter()
                .any(|def| token_of(def, settings) != def.default),
            max_fan_out_items: token(mp_contracts::org_agent_policy::RESEARCH_MAX_FAN_OUT_ITEMS)
                .parse()
                .unwrap_or(0),
        }
""", 1),
("""    #[test]
    fn org_agent_policy_mode_switch_reads_off_shadow_and_enforce() {
""", """    #[test]
    fn org_agent_policy_reads_the_wide_research_item_cap() {
        assert_eq!(OrgPolicy::from_settings(0, "", &settings(json!({}))).max_fan_out_items, 8);
        assert_eq!(
            OrgPolicy::from_settings(1, "d", &settings(json!({"research.max_fan_out_items": 3})))
                .max_fan_out_items,
            3
        );
        // A value this build cannot read turns wide research off for the org, never on.
        assert_eq!(
            OrgPolicy::from_settings(1, "d", &settings(json!({"research.max_fan_out_items": "x"})))
                .max_fan_out_items,
            0
        );
    }

    #[test]
    fn org_agent_policy_mode_switch_reads_off_shadow_and_enforce() {
""", 1),
])
edit('src/normalize.rs', [
("""            agent_autonomy_cap: None,
""", """            agent_autonomy_cap: None,
            wide_research: None,
""", 1),
])
