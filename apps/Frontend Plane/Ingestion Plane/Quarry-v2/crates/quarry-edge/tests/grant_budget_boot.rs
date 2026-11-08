//! Boot refusal for the grant budget claim switch (WS6.E).
//!
//! `QUARRY_EDGE__GRANT_BUDGET_CLAIM` decides whether a broker grant's budget claim
//! tightens the run it backs. A misspelt value must stop the edge at boot, naming the
//! variable, rather than leave the claim ignored while an operator believes it is on.
//! `ClaimMode::parse` is unit-tested, but the refusal is a line in `main`, so this
//! starts the real binary: replacing that `?` with a default makes it fail.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_quarry-edge-rs");
const VARIABLE: &str = "QUARRY_EDGE__GRANT_BUDGET_CLAIM";
const REFUSAL: &str = "must be ignore or enforce";

/// The edge, started with `value` in the claim variable and no other `QUARRY_*`
/// setting from the ambient environment, so nothing else decides the outcome.
fn spawn(value: &str) -> Child {
    let mut command = Command::new(BIN);
    for (name, _) in std::env::vars() {
        if name.starts_with("QUARRY") {
            command.env_remove(name);
        }
    }
    command
        .env(VARIABLE, value)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the edge binary starts")
}

/// Waits up to `limit` for the process to exit; kills it when it does not.
fn finish(mut child: Child, limit: Duration) -> (Option<ExitStatus>, String) {
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().expect("poll the edge") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let mut output = String::new();
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read>),
    ]
    .into_iter()
    .flatten()
    {
        let mut stream = stream;
        let mut text = String::new();
        let _ = stream.read_to_string(&mut text);
        output.push_str(&text);
    }
    (status, output)
}

#[test]
fn a_misspelt_claim_mode_stops_the_edge_at_boot_naming_the_variable() {
    for bad in ["onn", "true", "enforced", "1"] {
        let (status, output) = finish(spawn(bad), Duration::from_secs(60));
        let status = status.unwrap_or_else(|| panic!("{bad:?}: the edge kept running; output:\n{output}"));
        assert!(!status.success(), "{bad:?}: exited cleanly; output:\n{output}");
        assert!(
            output.contains(VARIABLE) && output.contains(REFUSAL),
            "{bad:?}: the refusal must name the variable and the accepted values; output:\n{output}"
        );
        assert!(
            output.contains(&format!("{bad:?}")),
            "{bad:?}: the refusal names the value it got; output:\n{output}"
        );
    }
}

#[test]
fn a_readable_claim_mode_gets_past_the_check() {
    // The control: the refusal above is about the value. Whatever else the edge does
    // with this environment, it must not refuse these for the claim switch.
    for good in ["enforce", "ignore", "", "Enforce"] {
        let (_, output) = finish(spawn(good), Duration::from_secs(4));
        assert!(
            !output.contains(REFUSAL),
            "{good:?} was refused as a claim mode; output:\n{output}"
        );
    }
}
