//! Section 6 of the coding-complete report: each of these W2-W5 switches is "Only the literal true". A value that is
//! the word true in another case must leave the feature off (surrounding whitespace is not judged here: the crate's
//! strict readers trim it too). This binary sets the variables itself and runs its one test alone, so no other test
//! sees them.

use model_gateway::{
    council_routes, event_trigger_routes, goal_unattended, reasoning_translate_routes,
    side_chat_routes,
};

#[test]
fn only_the_literal_true_turns_these_switches_on() {
    let readers: [(&str, fn() -> bool); 5] = [
        (
            "MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED",
            goal_unattended::enabled,
        ),
        (
            "MODEL_GATEWAY_MODEL_COUNCIL_ENABLED",
            council_routes::enabled,
        ),
        (
            "MODEL_GATEWAY_EVENT_TRIGGERS",
            event_trigger_routes::enabled,
        ),
        ("MODEL_GATEWAY_SIDE_CHAT_ENABLED", side_chat_routes::enabled),
        (
            "MODEL_GATEWAY_REASONING_TRANSLATE_ENABLED",
            reasoning_translate_routes::enabled,
        ),
    ];
    let mut loose = Vec::new();
    for (name, enabled) in readers {
        std::env::set_var(name, "true");
        assert!(enabled(), "{name}=true must turn it on");
        for value in ["TRUE", "True", "tRuE"] {
            std::env::set_var(name, value);
            if enabled() {
                loose.push(format!("{name}={value:?}"));
            }
        }
        std::env::remove_var(name);
        assert!(!enabled(), "{name} unset is off");
    }
    assert!(
        loose.is_empty(),
        "switches turned on by a value other than the literal true: {loose:?}"
    );
}
