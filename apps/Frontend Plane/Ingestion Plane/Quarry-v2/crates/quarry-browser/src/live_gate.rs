//! The one gate every live-Chromium test goes through.
//!
//! A live test needs a real Chromium. Without `CHROMIUMOXIDE_TEST=1` it prints
//! "skipping" and passes, which is right for a developer laptop and for the
//! ordinary workspace job. WITH `CHROMIUMOXIDE_TEST=1` the caller has said "this
//! run must exercise the browser", so a missing Chromium is a failure of the run,
//! not a reason to skip: a CI lane whose browser failed to install would
//! otherwise report green having tested nothing.
//!
//! The decision itself is a pure function ([`decide`]) so the table of cases is
//! tested without a browser. [`live_enabled`] is the thin wrapper the tests call;
//! it reads the environment, asks chromiumoxide's own detection what it would
//! launch, and panics (fails the test) in the one case that must not skip.
//!
//! `CHROME` is treated as an explicit choice. chromiumoxide itself ignores a
//! `CHROME` that points at nothing and quietly falls back to whatever `chromium`
//! is on the `PATH`; that would let a lane that pinned one browser run another
//! and pass. Here a `CHROME` that does not exist is an error.

use std::path::{Path, PathBuf};

/// The environment variable that turns the live tests on.
pub const ENV_FLAG: &str = "CHROMIUMOXIDE_TEST";

/// What the gate decided when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// The live tests are not requested; the test should return early.
    Skip,
    /// The live tests are requested and this is the browser that will launch.
    Run(PathBuf),
}

/// Decide whether a live test runs, skips or fails.
///
/// * `flag` is the value of `CHROMIUMOXIDE_TEST`; only the exact value `1` turns
///   the tests on.
/// * `chrome_env` is the value of `CHROME`; empty counts as unset.
/// * `path_exists` answers whether a path exists (injected so the table needs no
///   filesystem).
/// * `detect` is chromiumoxide's own executable detection, called only when
///   `CHROME` is unset.
///
/// Returns `Err(reason)` when the tests are requested and no Chromium is usable.
pub fn decide(
    flag: Option<&str>,
    chrome_env: Option<&str>,
    path_exists: impl Fn(&Path) -> bool,
    detect: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<Gate, String> {
    if flag != Some("1") {
        return Ok(Gate::Skip);
    }
    match chrome_env.filter(|value| !value.is_empty()) {
        Some(explicit) => {
            let path = PathBuf::from(explicit);
            if path_exists(&path) {
                Ok(Gate::Run(path))
            } else {
                Err(format!(
                    "CHROME is set to {explicit:?} but nothing exists there; \
                     refusing to fall back to another browser on the PATH"
                ))
            }
        }
        None => detect()
            .map(Gate::Run)
            .map_err(|why| format!("no Chromium was found ({why}); install one or set CHROME")),
    }
}

/// `true` when a live test should run, `false` when it should return early.
///
/// # Panics
///
/// When `CHROMIUMOXIDE_TEST=1` and no usable Chromium exists. That is the point:
/// the test fails instead of skipping.
pub fn live_enabled() -> bool {
    let flag = std::env::var(ENV_FLAG).ok();
    let chrome = std::env::var("CHROME").ok();
    let outcome = decide(
        flag.as_deref(),
        chrome.as_deref(),
        |path| path.exists(),
        || {
            ::chromiumoxide::detection::default_executable(
                ::chromiumoxide::detection::DetectionOptions::default(),
            )
        },
    );
    enforce(outcome)
}

/// Map a gate decision onto what a test does: `true` runs, `false` returns
/// early, and a refusal FAILS the test.
///
/// This is the one place a missing browser becomes a red run instead of a
/// skip, so it is its own function: the tests below call it with injected
/// outcomes (no environment mutation, which would race other tests) and pin
/// the panic. A change that turned the `Err` arm into a skip would otherwise
/// pass every test of [`decide`].
///
/// # Panics
///
/// On `Err`, with the reason in the message.
pub fn enforce(outcome: Result<Gate, String>) -> bool {
    match outcome {
        Ok(Gate::Run(_)) => true,
        Ok(Gate::Skip) => {
            eprintln!("skipping: set {ENV_FLAG}=1 to run");
            false
        }
        Err(reason) => panic!("{ENV_FLAG}=1 but the live browser cannot run: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exists_only(known: &'static str) -> impl Fn(&Path) -> bool {
        move |path| path == Path::new(known)
    }

    fn detected(path: &str) -> impl FnOnce() -> Result<PathBuf, String> + '_ {
        move || Ok(PathBuf::from(path))
    }

    fn not_detected() -> Result<PathBuf, String> {
        Err("Could not auto detect a chrome executable".to_owned())
    }

    #[test]
    fn without_the_flag_every_case_skips_even_with_no_browser() {
        for flag in [None, Some(""), Some("0"), Some("true"), Some("yes")] {
            assert_eq!(
                decide(flag, Some("/nonexistent"), |_| false, not_detected),
                Ok(Gate::Skip),
                "flag {flag:?} must not turn the live tests on"
            );
        }
    }

    #[test]
    fn decide_reports_an_error_when_the_flag_is_set_and_no_chromium_is_found() {
        let err = decide(Some("1"), None, |_| false, not_detected).unwrap_err();
        assert!(err.contains("no Chromium was found"), "{err}");
        assert!(err.contains("Could not auto detect"), "{err}");
    }

    #[test]
    fn an_empty_chrome_counts_as_unset() {
        assert_eq!(
            decide(
                Some("1"),
                Some(""),
                |_| false,
                detected("/usr/bin/chromium")
            ),
            Ok(Gate::Run(PathBuf::from("/usr/bin/chromium")))
        );
        assert!(decide(Some("1"), Some(""), |_| false, not_detected).is_err());
    }

    #[test]
    fn an_existing_chrome_is_the_browser_that_runs() {
        assert_eq!(
            decide(
                Some("1"),
                Some("/opt/pinned/chrome"),
                exists_only("/opt/pinned/chrome"),
                // Would be a different browser; must not be consulted.
                || panic!("detection must not run when CHROME exists"),
            ),
            Ok(Gate::Run(PathBuf::from("/opt/pinned/chrome")))
        );
    }

    #[test]
    fn a_chrome_that_does_not_exist_fails_even_when_another_browser_is_on_the_path() {
        // chromiumoxide would silently launch /usr/bin/chromium here. The lane
        // pinned a browser, so running a different one and passing is wrong.
        let err = decide(
            Some("1"),
            Some("/nonexistent"),
            |_| false,
            detected("/usr/bin/chromium"),
        )
        .unwrap_err();
        assert!(err.contains("/nonexistent"), "{err}");
        assert!(err.contains("refusing to fall back"), "{err}");
    }

    // The mapping from decision to test behaviour. These are what make "a
    // missing browser fails the run" a pinned property rather than a reading
    // of the code.

    #[test]
    #[should_panic(
        expected = "CHROMIUMOXIDE_TEST=1 but the live browser cannot run: no Chromium was found"
    )]
    fn enforce_fails_the_test_when_the_flag_is_set_and_no_chromium_is_found() {
        let outcome = decide(Some("1"), None, |_| false, not_detected);
        enforce(outcome);
    }

    #[test]
    #[should_panic(expected = "refusing to fall back")]
    fn enforce_fails_the_test_when_chrome_names_a_missing_file() {
        let outcome = decide(
            Some("1"),
            Some("/nonexistent"),
            |_| false,
            detected("/usr/bin/chromium"),
        );
        enforce(outcome);
    }

    #[test]
    fn enforce_skips_without_the_flag_and_runs_with_a_browser() {
        assert!(!enforce(decide(
            None,
            Some("/nonexistent"),
            |_| false,
            not_detected
        )));
        assert!(enforce(decide(
            Some("1"),
            None,
            |_| false,
            detected("/usr/bin/chromium")
        )));
        assert!(enforce(Ok(Gate::Run(PathBuf::from("/x")))));
        assert!(!enforce(Ok(Gate::Skip)));
    }

    #[test]
    fn without_chrome_the_detected_browser_is_used() {
        assert_eq!(
            decide(Some("1"), None, |_| false, detected("/usr/bin/chromium")),
            Ok(Gate::Run(PathBuf::from("/usr/bin/chromium")))
        );
    }
}
