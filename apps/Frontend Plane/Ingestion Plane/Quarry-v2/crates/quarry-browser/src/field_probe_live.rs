// WS12.S1: the field-facts probe against a real Chromium, on a `set_content`
// fixture. This file is `include!`d at the end of `mod tests` in chromiumoxide.rs,
// so it shares that module's helpers (`live_driver`, `live_session`, `live_enabled`).
//
// Run: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//      cargo test -p quarry-browser --features chromiumoxide ax_field_probe_live -- --ignored --nocapture
//
// NOT RUN when this was written (build-first rule). The Q10 fixture
// `tests/fixtures/ax-password-probe.json` is not produced yet either.

/// The canary is joined from two fragments so no secret-shaped literal is in the source.
const PROBE_CANARY: &str = concat!("S3cret", "P@ss-0451");

/// Fields the probe must classify: a password (value set), a second password
/// with no value, a card number, a phone number, an OTP field, and a plain
/// text input that must keep its value.
fn secrets_lab_html() -> String {
    format!(
        r#"<!doctype html><html><body>
<label for="pw">Password</label><input id="pw" type="password" value="{PROBE_CANARY}">
<label for="pw2">New password</label><input id="pw2" type="password">
<label for="cc">Card number</label><input id="cc" autocomplete="cc-number" value="4111111111111111">
<label for="tel">Phone</label><input id="tel" type="tel" value="+4799999999">
<label for="otp">Verification code</label><input id="otp" autocomplete="one-time-code">
<label for="plain">Visible</label><input id="plain" value="visible-text">
</body></html>"#,
        PROBE_CANARY = PROBE_CANARY
    )
}

#[tokio::test]
#[ignore = "WS12.S1 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn ax_field_probe_live() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session(&driver, "ws12s1-field-probe").await;
    driver.new_tab(&session, None).await.expect("blank tab");
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(secrets_lab_html())
        .await
        .expect("inject page");
    let started = std::time::Instant::now();
    let projection = driver
        .native_page_projection(&session)
        .await
        .expect("native projection");
    let elapsed = started.elapsed();
    let stamp = projection.redaction.clone().expect("redaction stamp");
    let serialized = serde_json::to_string(&(&projection.accessibility, &projection.targets))
        .expect("serialize projection");
    assert!(
        !serialized.contains("S3cretP@ss"),
        "the password value leaked: {serialized}"
    );
    assert!(
        !serialized.contains("4111111111111111"),
        "the card number leaked: {serialized}"
    );
    assert!(
        !serialized.contains("+4799999999"),
        "the phone number leaked: {serialized}"
    );
    assert_eq!(
        stamp.probe,
        quarry_core::redaction::ProbeState::Ok,
        "every editable control should be probed: {stamp:?}"
    );
    println!(
        "field probe: probe_ms={} elapsed={:?} values_masked={} by_rule={:?}",
        stamp.probe_ms, elapsed, stamp.values_masked, stamp.by_rule
    );
    driver.release(session).await.expect("release");
}
