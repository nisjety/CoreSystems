//! Switches and bounds on a live agent run that are decided by the operator.
//!
//! Today that is one switch: whether an agent action is refused while a person
//! holds the browser (`QUARRY_EDGE__TAKEOVER_ENFORCEMENT`). The run caps and the
//! reaper join it here (WS2.D).

pub const ENV_TAKEOVER_ENFORCEMENT: &str = "QUARRY_EDGE__TAKEOVER_ENFORCEMENT";

/// Parse the takeover-enforcement switch. Unset or empty is off (observe: log what
/// would be refused and refuse nothing). `1`/`true`/`on` enforce; `0`/`false`/`off`
/// observe; anything else is an error naming the variable, so a typo cannot
/// silently leave enforcement off.
pub fn parse_takeover_enforcement(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!(
            "{ENV_TAKEOVER_ENFORCEMENT} must be 0, 1, on, off, true or false, got {other:?}"
        )),
    }
}

/// Whether an agent action is refused while a person holds the browser.
/// Off by default until the planner side maps the refusal (WS11); a value that
/// cannot be read is treated as off here and stops the edge at boot.
pub fn takeover_enforced() -> bool {
    parse_takeover_enforcement(std::env::var(ENV_TAKEOVER_ENFORCEMENT).ok().as_deref())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takeover_enforcement_is_off_unless_asked() {
        assert_eq!(parse_takeover_enforcement(None), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("")), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("0")), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("OFF")), Ok(false));
        assert_eq!(parse_takeover_enforcement(Some("1")), Ok(true));
        assert_eq!(parse_takeover_enforcement(Some(" On ")), Ok(true));
    }

    #[test]
    fn a_misspelled_switch_is_an_error_naming_the_variable() {
        let error = parse_takeover_enforcement(Some("enforce")).unwrap_err();
        assert!(error.contains(ENV_TAKEOVER_ENFORCEMENT), "{error}");
    }
}
