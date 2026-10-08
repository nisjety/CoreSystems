//! The kill switch for agentic (Do mode) runs on the user's ChatGPT
//! subscription, defined once so its three readers cannot drift.
//!
//! A run on `execution-core`'s governed agent loop can be pinned to the
//! `openai-codex-subscription` provider. Three independently deployed services
//! each have to agree on whether that is allowed:
//!
//! * `model-gateway` admits the request (`normalize::normalize_stream`) and
//!   forwards the route on `RunAgentRequest` (fields 16 and 17);
//! * `execution-core` re-validates the route at the gRPC boundary and refuses
//!   to run the loop on anything but that route;
//! * `inference-core` advertises the `agentic` model feature, which the
//!   composer follows instead of hard-coding the provider.
//!
//! All three read the same variable through this module, with the same
//! spelling of "on", so a deployment cannot have one service enabled and
//! another silently disabled by a typo in a single `if`.
//!
//! # Off by default
//!
//! Only an explicit on-word (`1`, `true`, `on`, `yes`, `enabled`, any case)
//! turns the switch on. Unset, empty and unrecognised values are off, like
//! `MODEL_GATEWAY_DATED_POST_SECTION_ONLY`. Compose passes
//! `${AGENTIC_SUBSCRIPTION_ENABLED:-false}` to all three services.
//!
//! # Why the order of a rollout matters
//!
//! An `execution-core` built before `RunAgentRequest` fields 16 and 17 existed
//! skips them as unknown and runs the loop on its default route, which is a
//! paid provider. Nothing in the old binary can notice. So the switch is turned
//! on only once the new `execution-core` is the one running; the new binary
//! then cannot take that route for a subscription run, whatever its
//! `EXECUTION_AGENT_MODEL` says.
//!
//! The values are read when asked, not cached for the process, so a test that
//! sets them is not at the mercy of which test touched them first. A deployed
//! process never changes its environment, so the two are the same in
//! production.

/// Master switch. Off unless set to an explicit on-word.
pub const ENABLED_ENV: &str = "AGENTIC_SUBSCRIPTION_ENABLED";

/// Second switch: whether a subscription run may act, that is, run without
/// plan mode. Off unless set to an explicit on-word, and it grants nothing
/// while [`ENABLED_ENV`] is off. Until the plan-approval flow is connected a
/// subscription run is read-only by construction (plan mode is required).
pub const ALLOW_EFFECTS_ENV: &str = "AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS";

/// The canonical provider registry id of the ChatGPT subscription route. Not
/// an alias: the aliases (`codex-subscription`, `chatgpt-codex`) are accepted
/// at the gateway edge and canonicalised there, and `execution-core` accepts
/// only this spelling.
pub const PROVIDER_ID: &str = "openai-codex-subscription";

/// Stable error code of a request that asked for an agentic subscription run
/// while the switch is off.
pub const DISABLED_CODE: &str = "agentic_subscription_disabled";

/// Stable error code of a run that stopped because an inference round was not
/// served by the subscription route it was pinned to.
pub const ROUTE_UNAVAILABLE_CODE: &str = "subscription_route_unavailable";

/// Whether `raw` (the value of an environment variable, if set) is an explicit
/// on-word. Surrounding whitespace and case are ignored.
#[must_use]
pub fn switch_value_is_on(raw: Option<&str>) -> bool {
    raw.map(str::trim).is_some_and(|value| {
        ["1", "true", "on", "yes", "enabled"]
            .iter()
            .any(|on| value.eq_ignore_ascii_case(on))
    })
}

/// Both switches, as one value a pure function can take instead of reading the
/// environment itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    /// [`ENABLED_ENV`].
    pub enabled: bool,
    /// [`ALLOW_EFFECTS_ENV`]. Only meaningful together with `enabled`; use
    /// [`Switches::effects_permitted`] to read it.
    pub allow_effects: bool,
}

impl Switches {
    /// Everything off: what an unset environment means.
    pub const OFF: Self = Self {
        enabled: false,
        allow_effects: false,
    };

    /// The process environment right now.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var(ENABLED_ENV).ok().as_deref(),
            std::env::var(ALLOW_EFFECTS_ENV).ok().as_deref(),
        )
    }

    /// The switches those two raw values mean.
    #[must_use]
    pub fn from_values(enabled: Option<&str>, allow_effects: Option<&str>) -> Self {
        Self {
            enabled: switch_value_is_on(enabled),
            allow_effects: switch_value_is_on(allow_effects),
        }
    }

    /// Whether a subscription run may act rather than only plan. False
    /// whenever the master switch is off, whatever the second one says.
    #[must_use]
    pub fn effects_permitted(self) -> bool {
        self.enabled && self.allow_effects
    }
}

/// Whether the master switch is on in the process environment.
#[must_use]
pub fn enabled() -> bool {
    Switches::from_env().enabled
}

/// Whether `model` names no concrete model: empty, or one of the pseudo names
/// inference-core's intent layer resolves to a model of its own choosing
/// (`verevon`, `verevon-*`, `auto`, `default`).
///
/// A subscription run needs a concrete id. With a pseudo name the provider's
/// own default would be served and `model_used` would never equal what was
/// asked for, so the run would be stopped on its first round. Refusing the
/// name up front says why.
#[must_use]
pub fn is_pseudo_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.is_empty() || model == "auto" || model == "default" || model.starts_with("verevon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_on_word_turns_a_switch_on() {
        for on in ["1", "true", "on", "yes", "enabled", " TRUE ", "Enabled", "On"] {
            assert!(switch_value_is_on(Some(on)), "{on:?} must be on");
        }
        for off in [
            "", " ", "0", "false", "off", "no", "disabled", "2", "y", "tru", "enable",
            "yes please", "null",
        ] {
            assert!(!switch_value_is_on(Some(off)), "{off:?} must be off");
        }
        assert!(!switch_value_is_on(None), "an unset variable is off");
    }

    #[test]
    fn switches_default_to_off_and_effects_need_the_master_switch() {
        assert_eq!(Switches::from_values(None, None), Switches::OFF);
        assert_eq!(Switches::default(), Switches::OFF);
        assert!(!Switches::OFF.effects_permitted());

        let enabled = Switches::from_values(Some("1"), None);
        assert!(enabled.enabled);
        assert!(
            !enabled.effects_permitted(),
            "enabling the route does not allow effects"
        );

        let effects_only = Switches::from_values(None, Some("1"));
        assert!(
            !effects_only.effects_permitted(),
            "the effects switch grants nothing while the master switch is off"
        );

        let both = Switches::from_values(Some("true"), Some("yes"));
        assert!(both.enabled && both.effects_permitted());
    }

    #[test]
    fn pseudo_models_are_not_concrete_ids() {
        for pseudo in [
            "",
            "  ",
            "auto",
            "AUTO",
            "default",
            "verevon",
            "verevon-auto",
            "Verevon-Balance",
            " verevon-genius ",
            "verevon-budget",
        ] {
            assert!(is_pseudo_model(pseudo), "{pseudo:?} names no concrete model");
        }
        for concrete in ["gpt-5.6-terra", "gpt-6-astra", "gpt-5.3-codex-spark", "model-router"] {
            assert!(
                !is_pseudo_model(concrete),
                "{concrete:?} is a concrete id as far as this check goes"
            );
        }
    }

    #[test]
    fn the_codes_and_names_are_the_ones_the_other_services_and_the_composer_use() {
        // Spelled out so that renaming one of these is a deliberate edit that
        // fails here, not a silent drift between the three services and the
        // client that matches on the code.
        assert_eq!(ENABLED_ENV, "AGENTIC_SUBSCRIPTION_ENABLED");
        assert_eq!(ALLOW_EFFECTS_ENV, "AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS");
        assert_eq!(PROVIDER_ID, "openai-codex-subscription");
        assert_eq!(DISABLED_CODE, "agentic_subscription_disabled");
        assert_eq!(ROUTE_UNAVAILABLE_CODE, "subscription_route_unavailable");
    }
}
