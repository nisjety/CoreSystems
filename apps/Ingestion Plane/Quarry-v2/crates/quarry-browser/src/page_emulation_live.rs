// E4.4-a: the per-page locale, timezone and location. This file is `include!`d at the end of
// `mod tests` in chromiumoxide.rs, so it shares that module's helpers. The first test needs no
// browser. The live test is `#[ignore]`d and needs Chromium.
//
// Run the live test: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//      cargo test -p quarry-browser --features chromiumoxide page_emulation -- --ignored --nocapture
//
// NOT RUN when this was written: the build environment has no Chromium. The geolocation grant
// needs a public origin to be checked end to end, so the live test covers the locale, the timezone
// and the once-per-page refusal only.

#[tokio::test]
async fn page_emulation_is_kept_for_the_session_and_an_empty_one_keeps_nothing() {
    let driver = ChromiumoxideDriver::new();
    let session = BrowserSession {
        lease: make_lease_with_key("e44a-store"),
        tab_pin: None,
        inner: Arc::new(Mutex::new(SessionInner {
            connected: true,
            pages_served: 0,
        })),
    };
    let key = ChromiumoxideDriver::session_key(&session);
    let emulation = quarry_core::page_emulation::PageEmulation {
        locale: Some("nb_NO".to_owned()),
        timezone: Some("Europe/Oslo".to_owned()),
        location: None,
    };
    driver
        .set_page_emulation(&session, &emulation)
        .await
        .expect("set before the first page");
    let stored = driver
        .pages
        .lock()
        .await
        .get(&key)
        .and_then(|pages| pages.page_emulation.clone());
    assert_eq!(stored, Some(emulation));
    driver
        .set_page_emulation(
            &session,
            &quarry_core::page_emulation::PageEmulation::default(),
        )
        .await
        .expect("an empty emulation is accepted");
    let stored = driver
        .pages
        .lock()
        .await
        .get(&key)
        .and_then(|pages| pages.page_emulation.clone());
    assert_eq!(stored, None);
}

#[tokio::test]
#[ignore = "E4.4-a live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn page_emulation_sets_the_locale_and_timezone_of_the_first_page() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = driver
        .acquire(&make_lease_with_key("e44a-emulation"))
        .await
        .expect("acquire");
    driver
        .set_page_emulation(
            &session,
            &quarry_core::page_emulation::PageEmulation {
                locale: Some("nb_NO".to_owned()),
                timezone: Some("Europe/Oslo".to_owned()),
                location: None,
            },
        )
        .await
        .expect("emulation before the first page");
    driver
        .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
        .await
        .expect("install deny-all egress policy");
    driver.new_tab(&session, None).await.expect("first tab");
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content("<html><body>emulated</body></html>")
        .await
        .expect("fixture");
    assert_eq!(
        eval_string(&driver, &session, "navigator.language").await,
        "nb-NO"
    );
    assert_eq!(
        eval_string(
            &driver,
            &session,
            "Intl.DateTimeFormat().resolvedOptions().timeZone"
        )
        .await,
        "Europe/Oslo"
    );
    // The page is open now: its emulation can no longer be set.
    let refused = driver
        .set_page_emulation(
            &session,
            &quarry_core::page_emulation::PageEmulation {
                timezone: Some("UTC".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect_err("a page is already open");
    assert_eq!(
        refused
            .details
            .as_ref()
            .and_then(|details| details.get("reason"))
            .and_then(|reason| reason.as_str()),
        Some("page_already_open")
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "E4.4-a live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn page_emulation_refuses_a_timezone_the_browser_rejects() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = driver
        .acquire(&make_lease_with_key("e44a-bad-zone"))
        .await
        .expect("acquire");
    driver
        .set_page_emulation(
            &session,
            &quarry_core::page_emulation::PageEmulation {
                timezone: Some("Mars/Olympus".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect("stored before the page opens");
    driver
        .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
        .await
        .expect("install deny-all egress policy");
    let error = driver
        .new_tab(&session, None)
        .await
        .expect_err("the browser refuses the zone");
    assert_eq!(error.code, ErrorCode::BadRequest);
    assert_eq!(
        error
            .details
            .as_ref()
            .and_then(|details| details.get("field"))
            .and_then(|field| field.as_str()),
        Some("timezone")
    );
    driver.release(session).await.expect("release");
}
