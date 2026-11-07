//! The Do loop's computer tools (W4-C C1): `computer.observe` and `computer.act`, typed and dark.
//!
//! A computer action is named from a closed list, and the element it is for is named by a reference an
//! observation gave, never by a position. [`ComputerAct`] is that list. Each action takes exactly the fields
//! it names, so a coordinate (`x`, `y`, `coordinates`, `position`) is a schema failure, not a value that is
//! quietly dropped.
//!
//! Nothing serves a call yet, because no computer backend is built (F-t3-024). So every call is refused. A
//! zero-retention run gets the feature refusal (`tool_execution`) whatever the switch says. Any other run gets
//! `computer_unavailable` while `EXECUTION_DO_COMPUTER_ENABLED` is off, and still does when it is on, because
//! the switch says the deployment wants the tools and nothing can run them. With the switch on, a call the
//! schema refuses is told `computer_invalid_input` instead. Neither tool is offered to a model in this slice,
//! and the contract test holds that.
//!
//! When a backend lands, every `computer.act` must pause for the person's approval whatever the permission
//! mode, as `browser.act` does (WS21-S5b). The seed's approval floor stays `none`: the parity test holds the
//! seed to the name heuristics, and those allow `computer.act` under `auto`. The pause is the executor's to add.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;

/// The computer tools by name.
pub(crate) const OBSERVE_TOOL: &str = "computer.observe";
pub(crate) const ACT_TOOL: &str = "computer.act";

/// The deployment switch. Only the literal `true` turns it on, and the environment is read once.
const SWITCH_ENV: &str = "EXECUTION_DO_COMPUTER_ENABLED";
/// The feature a zero-retention refusal names: the catalogue's key for tool execution. A key of its own would
/// mean regenerating the refusal catalogue in every plane that vendors it.
const ZDR_FEATURE: &str = "tool_execution";
/// The longest element reference, and the longest text one `type` action carries.
const MAX_REF_CHARS: usize = 32;
const MAX_TEXT_CHARS: usize = 1000;

/// A refusal the model reads: a closed code, and a fixed sentence that names no id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

impl Refusal {
    /// The text a failed tool call carries: the code, then the message.
    #[must_use]
    pub(crate) fn text(self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

/// The computer is not served: the switch is off, or it is on and no backend serves the call yet.
pub(crate) const UNAVAILABLE: Refusal = Refusal {
    code: "computer_unavailable",
    message: "The computer tools are not available right now.",
};

/// The input does not match the closed schema of its tool.
pub(crate) const INVALID_INPUT: Refusal = Refusal {
    code: "computer_invalid_input",
    message: "The computer tool input does not match its schema.",
};

/// Whether `name` is one of the computer tools. Dispatch asks this; the offer does not list them.
#[must_use]
pub(crate) fn is_computer_tool(name: &str) -> bool {
    matches!(name, OBSERVE_TOOL | ACT_TOOL)
}

/// A computer call, read against the closed schema of its tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComputerCall {
    Observe,
    Act(ComputerAct),
}

/// One action on the computer, from the closed list the Do loop accepts. Each variant names the element it is
/// for by reference, and no variant has a position. The schema refuses an unknown field, so a coordinate is an
/// error and never a value that is dropped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ComputerAct {
    /// Click the element the reference names.
    Click { target: ElementRef },
    /// Type text into the element the reference names.
    Type { target: ElementRef, text: TypedText },
    /// Press one key from the closed list.
    Press { key: Key },
    /// Scroll the page one way.
    Scroll { direction: Direction },
}

/// The reference an observation gave an element: 1 to 32 ASCII letters, digits, `-` or `_`. It names an
/// element, not a point, so no reference says where on the screen an action lands.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct ElementRef(String);

impl TryFrom<String> for ElementRef {
    type Error = &'static str;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let well_formed = (1..=MAX_REF_CHARS).contains(&raw.len())
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if well_formed {
            Ok(Self(raw))
        } else {
            Err("an element reference is 1 to 32 letters, digits, '-' or '_'")
        }
    }
}

/// The text a `type` action enters: at most 1000 characters, with no control character. Enter is a [`Key`],
/// not a character of the text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct TypedText(String);

impl TryFrom<String> for TypedText {
    type Error = &'static str;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        if raw.chars().count() <= MAX_TEXT_CHARS && !raw.chars().any(char::is_control) {
            Ok(Self(raw))
        } else {
            Err("typed text is at most 1000 characters, with no control character")
        }
    }
}

/// A key a `press` action may name. Closed: any other key is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Key {
    Enter,
    Tab,
    Escape,
    Backspace,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

/// The way a `scroll` action moves the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Up,
    Down,
}

/// Reads a call's input against the closed schema of `tool_name`. An empty input is an empty object. The
/// caller has checked the name with [`is_computer_tool`].
pub(crate) fn read_call(tool_name: &str, tool_input: &str) -> Result<ComputerCall, Refusal> {
    let input = if tool_input.trim().is_empty() {
        "{}"
    } else {
        tool_input
    };
    // Both schemas take one JSON object. A derived enum or struct would also take an array as its fields in
    // order (`["click","e1"]`), so the object is checked before any field is read.
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(input) else {
        return Err(INVALID_INPUT);
    };
    match tool_name {
        OBSERVE_TOOL if fields.is_empty() => Ok(ComputerCall::Observe),
        OBSERVE_TOOL => Err(INVALID_INPUT),
        ACT_TOOL => serde_json::from_value::<ComputerAct>(Value::Object(fields))
            .map(ComputerCall::Act)
            .map_err(|_| INVALID_INPUT),
        _ => Err(UNAVAILABLE),
    }
}

/// The text a computer call is answered with, which a failed tool call carries as its error. A zero-retention
/// run is refused by the feature whatever the switch says. Any other run is refused `computer_unavailable`
/// while the switch is off. With it on, a call the schema refuses is `computer_invalid_input`, and a call the
/// schema accepts is still `computer_unavailable`, because no computer backend serves it yet.
#[must_use]
pub(crate) fn refusal_text(
    tool_name: &str,
    tool_input: &str,
    zdr: bool,
    switched_on: bool,
) -> String {
    if zdr {
        let refusal = mp_contracts::zdr_refusal::Refusal::feature_not_supported(ZDR_FEATURE);
        return format!("{}: {}", refusal.code(), refusal.message());
    }
    if !switched_on {
        return UNAVAILABLE.text();
    }
    match read_call(tool_name, tool_input) {
        Err(refusal) => refusal.text(),
        Ok(_) => UNAVAILABLE.text(),
    }
}

/// Whether the deployment has switched the computer tools on. Only the literal `true` turns them on, and the
/// environment is read once, not on every call.
#[must_use]
pub(crate) fn computer_enabled() -> bool {
    static SWITCHED_ON: OnceLock<bool> = OnceLock::new();
    *SWITCHED_ON.get_or_init(|| parse_switch(std::env::var(SWITCH_ENV).ok().as_deref()))
}

fn parse_switch(raw: Option<&str>) -> bool {
    raw == Some("true")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(target: &str) -> String {
        format!(r#"{{"action":"click","target":"{target}"}}"#)
    }

    #[test]
    fn the_switch_is_on_only_for_the_literal_true() {
        assert!(parse_switch(Some("true")));
        for raw in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some(" true"),
            Some("false"),
            Some("yes"),
        ] {
            assert!(
                !parse_switch(raw),
                "{raw:?} must leave the computer tools off"
            );
        }
    }

    #[test]
    fn only_the_two_names_are_computer_tools() {
        assert!(is_computer_tool("computer.observe"));
        assert!(is_computer_tool("computer.act"));
        for name in [
            "computer",
            "computer.open",
            "computer.act ",
            "Computer.act",
            "browser.act",
        ] {
            assert!(!is_computer_tool(name), "{name:?} is not a computer tool");
        }
    }

    #[test]
    fn observe_takes_no_input() {
        assert_eq!(read_call(OBSERVE_TOOL, ""), Ok(ComputerCall::Observe));
        assert_eq!(read_call(OBSERVE_TOOL, "{}"), Ok(ComputerCall::Observe));
        for input in [r#"{"target":"e1"}"#, r#"{"x":1,"y":2}"#, "null", "[]"] {
            assert_eq!(
                read_call(OBSERVE_TOOL, input),
                Err(INVALID_INPUT),
                "{input}"
            );
        }
    }

    #[test]
    fn each_action_names_its_element_by_reference() {
        let reference = |raw: &str| ElementRef(raw.to_owned());
        assert_eq!(
            read_call(ACT_TOOL, &click("e12")),
            Ok(ComputerCall::Act(ComputerAct::Click {
                target: reference("e12")
            }))
        );
        assert_eq!(
            read_call(
                ACT_TOOL,
                r#"{"action":"type","target":"e7","text":"hello"}"#
            ),
            Ok(ComputerCall::Act(ComputerAct::Type {
                target: reference("e7"),
                text: TypedText("hello".to_owned()),
            }))
        );
        assert_eq!(
            read_call(ACT_TOOL, r#"{"action":"press","key":"enter"}"#),
            Ok(ComputerCall::Act(ComputerAct::Press { key: Key::Enter }))
        );
        assert_eq!(
            read_call(ACT_TOOL, r#"{"action":"scroll","direction":"down"}"#),
            Ok(ComputerCall::Act(ComputerAct::Scroll {
                direction: Direction::Down
            }))
        );
    }

    #[test]
    fn a_coordinate_is_refused_wherever_it_is_put() {
        for input in [
            r#"{"action":"click","x":10,"y":20}"#,
            r#"{"action":"click","target":"e1","x":10}"#,
            r#"{"action":"click","target":{"x":10,"y":20}}"#,
            r#"{"action":"click","coordinates":[10,20]}"#,
            r#"{"action":"click","position":"10,20"}"#,
            r#"{"action":"type","target":"e1","text":"a","y":3}"#,
            r#"{"action":"press","key":"enter","x":3}"#,
            r#"{"action":"scroll","direction":"down","amount":120}"#,
        ] {
            assert_eq!(read_call(ACT_TOOL, input), Err(INVALID_INPUT), "{input}");
        }
    }

    #[test]
    fn the_action_list_is_closed() {
        for input in [
            r#"{"action":"drag","target":"e1"}"#,
            r#"{"action":"right_click","target":"e1"}"#,
            r#"{"action":"double_click","target":"e1"}"#,
            r#"{"action":"screenshot"}"#,
            r#"{"action":"Click","target":"e1"}"#,
            r#"{"action":"click"}"#,
            r#"{"target":"e1"}"#,
            r#"["click","e1"]"#,
            "null",
            "{}",
            "",
        ] {
            assert_eq!(read_call(ACT_TOOL, input), Err(INVALID_INPUT), "{input}");
        }
    }

    #[test]
    fn an_element_reference_is_a_short_word_the_observation_gave() {
        let longest = "a".repeat(MAX_REF_CHARS);
        for target in ["e1", "save-button_2", longest.as_str()] {
            assert!(read_call(ACT_TOOL, &click(target)).is_ok(), "{target}");
        }
        let too_long = "a".repeat(MAX_REF_CHARS + 1);
        for target in ["", too_long.as_str(), "a b", "a/b", "e.1", "1,2", "é"] {
            assert_eq!(
                read_call(ACT_TOOL, &click(target)),
                Err(INVALID_INPUT),
                "{target:?}"
            );
        }
    }

    #[test]
    fn typed_text_is_bounded_and_holds_no_control_character() {
        let typed = |text: &str| format!(r#"{{"action":"type","target":"e1","text":{text}}}"#);
        let full = format!(r#""{}""#, "a".repeat(MAX_TEXT_CHARS));
        let over = format!(r#""{}""#, "a".repeat(MAX_TEXT_CHARS + 1));
        assert!(read_call(ACT_TOOL, &typed(&full)).is_ok());
        assert!(read_call(ACT_TOOL, &typed(r#""""#)).is_ok());
        assert_eq!(read_call(ACT_TOOL, &typed(&over)), Err(INVALID_INPUT));
        for text in [r#""a\nb""#, r#""a\tb""#, r#""a\u0007b""#] {
            assert_eq!(
                read_call(ACT_TOOL, &typed(text)),
                Err(INVALID_INPUT),
                "{text}"
            );
        }
    }

    #[test]
    fn keys_and_directions_are_closed_lists() {
        let press = |key: &str| format!(r#"{{"action":"press","key":"{key}"}}"#);
        for key in [
            "enter",
            "tab",
            "escape",
            "backspace",
            "arrow_up",
            "arrow_down",
            "arrow_left",
            "arrow_right",
        ] {
            assert!(read_call(ACT_TOOL, &press(key)).is_ok(), "{key}");
        }
        for key in ["f5", "ctrl+c", "page_down", "Enter", "space"] {
            assert_eq!(
                read_call(ACT_TOOL, &press(key)),
                Err(INVALID_INPUT),
                "{key}"
            );
        }
        let scroll =
            |direction: &str| format!(r#"{{"action":"scroll","direction":"{direction}"}}"#);
        for direction in ["up", "down"] {
            assert!(
                read_call(ACT_TOOL, &scroll(direction)).is_ok(),
                "{direction}"
            );
        }
        for direction in ["left", "right", "Down"] {
            assert_eq!(
                read_call(ACT_TOOL, &scroll(direction)),
                Err(INVALID_INPUT),
                "{direction}"
            );
        }
    }

    #[test]
    fn a_switched_off_run_is_refused_unavailable_whatever_the_call() {
        let click_e1 = click("e1");
        for (name, input) in [
            (OBSERVE_TOOL, "{}"),
            (ACT_TOOL, click_e1.as_str()),
            (ACT_TOOL, r#"{"x":1}"#),
        ] {
            assert_eq!(
                refusal_text(name, input, false, false),
                UNAVAILABLE.text(),
                "{name} {input}"
            );
        }
    }

    #[test]
    fn a_switched_on_run_is_still_refused_unavailable_because_no_backend_serves_it() {
        let click_e1 = click("e1");
        assert_eq!(
            refusal_text(OBSERVE_TOOL, "{}", false, true),
            UNAVAILABLE.text()
        );
        assert_eq!(
            refusal_text(ACT_TOOL, &click_e1, false, true),
            UNAVAILABLE.text()
        );
    }

    #[test]
    fn a_switched_on_run_is_told_when_its_input_breaks_the_schema() {
        assert_eq!(
            refusal_text(ACT_TOOL, r#"{"action":"click","x":1,"y":2}"#, false, true),
            INVALID_INPUT.text()
        );
        assert_eq!(
            refusal_text(OBSERVE_TOOL, r#"{"target":"e1"}"#, false, true),
            INVALID_INPUT.text()
        );
    }

    #[test]
    fn zero_retention_is_refused_with_the_feature_whatever_the_switch_says() {
        let feature = mp_contracts::zdr_refusal::Refusal::feature_not_supported(ZDR_FEATURE);
        let typed = format!("{}: {}", feature.code(), feature.message());
        assert!(typed.starts_with("zdr_feature_not_supported: "));
        let click_e1 = click("e1");
        for switched_on in [false, true] {
            assert_eq!(refusal_text(ACT_TOOL, &click_e1, true, switched_on), typed);
            assert_eq!(refusal_text(OBSERVE_TOOL, "{}", true, switched_on), typed);
        }
    }
}
