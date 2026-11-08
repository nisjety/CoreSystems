// WS3.K: the trusted-input pipeline against a real Chromium, on `set_content`
// fixtures. This file is `include!`d at the end of `mod tests` in
// chromiumoxide.rs, so it shares that module's helpers (`live_driver`,
// `live_session`, `live_enabled`) and its private view of the driver.
//
// Run: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//      cargo test -p quarry-browser --features chromiumoxide -- --test-threads=1

/// A driver (trusted input on or off) with one session whose page is `html`.
async fn trusted_fixture(
    trusted: bool,
    key: &str,
    html: &str,
) -> (ChromiumoxideDriver, BrowserSession) {
    let driver = live_driver().with_trusted_input(trusted);
    let session = live_session(&driver, key).await;
    driver.new_tab(&session, None).await.expect("blank tab");
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(html).await.expect("inject page");
    (driver, session)
}

/// A JavaScript expression's value, as JSON (`null` for undefined).
async fn page_json(
    driver: &ChromiumoxideDriver,
    session: &BrowserSession,
    expression: &str,
) -> serde_json::Value {
    let page = driver.current_page(session).await.expect("current page");
    let text = page
        .evaluate(format!("JSON.stringify(({expression}) ?? null)"))
        .await
        .expect("evaluate")
        .into_value::<String>()
        .expect("json text");
    serde_json::from_str(&text).expect("json")
}

async fn run_in_page(driver: &ChromiumoxideDriver, session: &BrowserSession, script: &str) {
    let page = driver.current_page(session).await.expect("current page");
    page.evaluate(script.to_owned()).await.expect("evaluate");
}

/// The native target of the accessible element called `name`.
async fn ref_named(
    driver: &ChromiumoxideDriver,
    session: &BrowserSession,
    name: &str,
) -> BrowserNativeTarget {
    let projection = driver
        .native_page_projection(session)
        .await
        .expect("native projection");
    let names: Vec<_> = projection
        .targets
        .iter()
        .map(|target| target.name.clone())
        .collect();
    projection
        .targets
        .into_iter()
        .find(|target| target.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no actionable node named {name}: {names:?}"))
}

/// The native target of the node a CSS selector finds. For controls the
/// accessibility tree does not expose as one node (a date or colour input is
/// shown as sub-fields or a colour well), which a ref cannot name here.
async fn target_of_selector(
    driver: &ChromiumoxideDriver,
    session: &BrowserSession,
    selector: &str,
) -> BrowserNativeTarget {
    let page = driver.current_page(session).await.expect("current page");
    let element = page.find_element(selector).await.expect("element");
    BrowserNativeTarget {
        ax_node_id: format!("selector:{selector}"),
        backend_node_id: *element.backend_node_id.inner(),
        frame_id: None,
        role: None,
        name: None,
        value: None,
        field_kind: None,
    }
}

fn target_action(
    target: BrowserNativeTarget,
    operation: VerifiedTargetOperation,
) -> VerifiedTargetAction {
    VerifiedTargetAction {
        native_target: Some(target),
        selector: String::new(),
        tag: String::new(),
        attributes: Vec::new(),
        normalized_text: String::new(),
        operation,
    }
}

/// Resolve `name` now and run `operation` on it.
async fn act_on(
    driver: &ChromiumoxideDriver,
    session: &BrowserSession,
    name: &str,
    operation: VerifiedTargetOperation,
) -> QuarryResult<()> {
    let target = ref_named(driver, session, name).await;
    driver
        .act_on_verified_target(session, target_action(target, operation))
        .await
}

fn assert_repair(error: &QuarryError, reason: &str) {
    assert_eq!(error.code, ErrorCode::TargetRepairRequired, "{error:?}");
    assert_eq!(error.code.http_status(), 409);
    assert_eq!(error.target_repair_reason(), Some(reason), "{error:?}");
}

#[test]
fn driver_capability_follows_the_flag() {
    assert!(
        !ChromiumoxideDriver::new()
            .with_trusted_input(false)
            .capabilities()
            .trusted_input
    );
    let on = ChromiumoxideDriver::new().with_trusted_input(true);
    assert!(on.capabilities().trusted_input);
    // Turning it on adds nothing else and removes nothing.
    assert!(on.capabilities().atomic_target_actions);
    // The background views of the driver see the same flag.
    assert!(on.share().capabilities().trusted_input);
}

#[tokio::test]
async fn trusted_click_has_istrusted_true() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-click",
        "<button id=b onclick=\"window.__r={t:event.isTrusted,n:(window.__n=(window.__n||0)+1)}\">Go</button>",
    )
    .await;
    act_on(&driver, &session, "Go", VerifiedTargetOperation::Click)
        .await
        .expect("click");
    let seen = page_json(&driver, &session, "window.__r").await;
    assert_eq!(seen["t"], true, "the click arrived as real input: {seen}");
    assert_eq!(seen["n"], 1, "exactly one click");
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn flag_off_clicks_synthetically_and_ignores_the_overlay() {
    if !live_enabled() {
        return;
    }
    // Same page as the covered test, flag off: byte-for-byte the old behaviour.
    let (driver, session) = trusted_fixture(
        false,
        "ti-off",
        "<button id=b style=\"position:absolute;left:20px;top:20px\" onclick=\"window.__r={t:event.isTrusted}\">Buy</button>\
         <div id=ov style=\"position:fixed;left:0;top:0;right:0;bottom:0\"></div>",
    )
    .await;
    assert!(!driver.capabilities().trusted_input);
    act_on(&driver, &session, "Buy", VerifiedTargetOperation::Click)
        .await
        .expect("a synthetic click reaches the covered button");
    assert_eq!(
        page_json(&driver, &session, "window.__r").await["t"],
        false,
        "flag off: the in-page click, not real input"
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn covered_button_fails_target_covered_with_tag() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-covered",
        "<button id=b style=\"position:absolute;left:20px;top:20px\" onclick=\"window.__r={t:event.isTrusted,n:(window.__n=(window.__n||0)+1)}\">Buy</button>\
         <div id=ov style=\"position:fixed;left:0;top:0;right:0;bottom:0;background:rgba(0,0,0,.3)\">Accept cookies</div>",
    )
    .await;
    let started = Instant::now();
    let error = act_on(&driver, &session, "Buy", VerifiedTargetOperation::Click)
        .await
        .expect_err("an overlay covers the button");
    assert_repair(&error, "target_covered");
    let details = error.details.clone().expect("details");
    assert_eq!(details["covered_by_tag"], "div");
    assert_eq!(
        details.as_object().expect("object").len(),
        2,
        "reason and tag only: {details}"
    );
    let rendered = format!("{error:?}");
    assert!(
        !rendered.contains("Accept cookies") && !rendered.contains("Buy"),
        "no page text in the error: {rendered}"
    );
    // The wait is bounded by Op::Actionability (2 s), not by the 10 s native action.
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        page_json(&driver, &session, "window.__n").await,
        serde_json::Value::Null,
        "nothing was clicked"
    );

    // Dismiss the overlay: the same ref now works, with real input.
    run_in_page(&driver, &session, "document.getElementById('ov').remove()").await;
    act_on(&driver, &session, "Buy", VerifiedTargetOperation::Click)
        .await
        .expect("click after dismissal");
    let seen = page_json(&driver, &session, "window.__r").await;
    assert_eq!(
        (seen["t"].clone(), seen["n"].clone()),
        (true.into(), 1.into())
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn offscreen_button_is_scrolled_then_clicked() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-offscreen",
        "<div style=\"height:4000px\"></div>\
         <button id=b onclick=\"window.__r={t:event.isTrusted,y:window.scrollY}\">Far</button>",
    )
    .await;
    assert_eq!(page_json(&driver, &session, "window.scrollY").await, 0);
    act_on(&driver, &session, "Far", VerifiedTargetOperation::Click)
        .await
        .expect("click after scrolling");
    let seen = page_json(&driver, &session, "window.__r").await;
    assert_eq!(seen["t"], true);
    assert!(
        seen["y"].as_f64().expect("scrollY") > 1000.0,
        "the page scrolled to the button: {seen}"
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn disabled_button_fails_target_disabled() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-disabled",
        "<button id=b disabled onclick=\"window.__n=1\">Off</button>\
         <button id=c aria-disabled=\"true\" onclick=\"window.__m=1\">Soft</button>",
    )
    .await;
    for name in ["Off", "Soft"] {
        let error = act_on(&driver, &session, name, VerifiedTargetOperation::Click)
            .await
            .expect_err("disabled");
        assert_repair(&error, "target_disabled");
    }
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn display_none_fails_target_not_visible() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-hidden",
        "<button id=b onclick=\"window.__n=1\">Shy</button><button id=c onclick=\"window.__n=2\">Gone</button>",
    )
    .await;
    // Observed while visible, then hidden: the ref names a node that exists but
    // cannot be pointed at. (A node that is display:none at observation time
    // has no ref at all.)
    let shy = ref_named(&driver, &session, "Shy").await;
    let gone = ref_named(&driver, &session, "Gone").await;
    run_in_page(
        &driver,
        &session,
        "document.getElementById('b').style.display='none';document.getElementById('c').remove();",
    )
    .await;
    let error = driver
        .act_on_verified_target(&session, target_action(shy, VerifiedTargetOperation::Click))
        .await
        .expect_err("hidden");
    assert_repair(&error, "target_not_visible");
    // A node that left the document still resolves (its backend node id is
    // kept), and the probe then finds it detached: target_not_attached.
    let error = driver
        .act_on_verified_target(
            &session,
            target_action(gone, VerifiedTargetOperation::Click),
        )
        .await
        .expect_err("removed");
    assert_eq!(error.code, ErrorCode::TargetRepairRequired, "{error:?}");
    assert_repair(&error, "target_not_attached");
    assert_eq!(
        error
            .details
            .as_ref()
            .expect("details")
            .as_object()
            .expect("object")
            .len(),
        1,
        "the reason only: {error:?}"
    );
    assert_eq!(
        page_json(&driver, &session, "window.__n").await,
        serde_json::Value::Null
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn moving_button_fails_unstable_then_succeeds_once_settled() {
    if !live_enabled() {
        return;
    }
    // 2000 px in 20 s: 2.5 px per 25 ms sample, never still, no turnaround.
    let (driver, session) = trusted_fixture(
        true,
        "ti-moving",
        "<style>@keyframes slide{from{transform:translateX(0)}to{transform:translateX(2000px)}}\
         #b{position:absolute;left:10px;top:50px;animation:slide 20s linear forwards}</style>\
         <button id=b onclick=\"window.__r={t:event.isTrusted}\">Move</button>",
    )
    .await;
    let started = Instant::now();
    let error = act_on(&driver, &session, "Move", VerifiedTargetOperation::Click)
        .await
        .expect_err("a moving target is not clicked");
    assert_repair(&error, "target_unstable");
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_secs(6),
        "polled for the actionability budget: {waited:?}"
    );
    assert_eq!(
        page_json(&driver, &session, "window.__r").await,
        serde_json::Value::Null
    );
    run_in_page(
        &driver,
        &session,
        "const b=document.getElementById('b');b.style.animation='none';b.style.transform='none';",
    )
    .await;
    act_on(&driver, &session, "Move", VerifiedTargetOperation::Click)
        .await
        .expect("click once settled");
    assert_eq!(page_json(&driver, &session, "window.__r.t").await, true);
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn a_target_that_settles_while_waiting_is_clicked() {
    if !live_enabled() {
        return;
    }
    // The wait is a poll, not a one-shot check: a button that becomes enabled
    // during the budget is clicked.
    let (driver, session) = trusted_fixture(
        true,
        "ti-settles",
        "<button id=b disabled onclick=\"window.__r={t:event.isTrusted}\">Later</button>\
         <script>setTimeout(()=>document.getElementById('b').disabled=false,600)</script>",
    )
    .await;
    act_on(&driver, &session, "Later", VerifiedTargetOperation::Click)
        .await
        .expect("click after it became enabled");
    assert_eq!(page_json(&driver, &session, "window.__r.t").await, true);
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn type_replaces_value_and_emits_trusted_input_event() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-type",
        "<input id=i aria-label=\"Name\" value=\"old text\">\
         <input id=r aria-label=\"Locked\" readonly value=\"fixed\">\
         <script>const i=document.getElementById('i');window.__ev=[];\
         for (const t of ['beforeinput','input','change']) \
           i.addEventListener(t,e=>window.__ev.push([e.type,e.isTrusted,i.value]));</script>",
    )
    .await;
    let typed = |text: &str| VerifiedTargetOperation::Type { text: text.into() };
    act_on(&driver, &session, "Name", typed("new text"))
        .await
        .expect("type");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('i').value").await,
        "new text",
        "the old value was replaced, not appended to"
    );
    let events = page_json(&driver, &session, "window.__ev").await;
    let events = events.as_array().expect("events");
    let input = events
        .iter()
        .find(|event| event[0] == "input")
        .unwrap_or_else(|| panic!("no input event: {events:?}"));
    assert_eq!(input[1], true, "the input event is trusted: {events:?}");
    assert_eq!(input[2], "new text");
    assert!(
        events
            .iter()
            .any(|event| event[0] == "beforeinput" && event[1] == true),
        "{events:?}"
    );
    // An empty text clears the field.
    act_on(&driver, &session, "Name", typed(""))
        .await
        .expect("clear");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('i').value").await,
        ""
    );
    // A read-only field is not typed into.
    let error = act_on(&driver, &session, "Locked", typed("changed"))
        .await
        .expect_err("read-only");
    assert_repair(&error, "target_disabled");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('r').value").await,
        "fixed"
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn select_passes_the_gate_then_sets_the_option() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-select",
        "<select id=s aria-label=\"Pick\"><option value=a>A</option><option value=b>B</option></select>\
         <select id=d aria-label=\"Locked\" disabled><option value=a>A</option></select>\
         <script>window.__ev=[];document.getElementById('s').addEventListener('change',e=>window.__ev.push(e.type));</script>",
    )
    .await;
    let pick = |value: &str| VerifiedTargetOperation::Select {
        value: value.into(),
    };
    act_on(&driver, &session, "Pick", pick("b"))
        .await
        .expect("select");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('s').value").await,
        "b"
    );
    assert_eq!(
        page_json(&driver, &session, "window.__ev").await,
        serde_json::json!(["change"])
    );
    // An option that does not exist is the old in-page reason, not a closed one.
    let error = act_on(&driver, &session, "Pick", pick("zzz"))
        .await
        .expect_err("no such option");
    assert_eq!(error.code, ErrorCode::TargetRepairRequired);
    assert_eq!(
        error.details.as_ref().expect("details")["reason"],
        "select_value_unavailable"
    );
    // A disabled select fails the gate.
    let error = act_on(&driver, &session, "Locked", pick("a"))
        .await
        .expect_err("disabled");
    assert_repair(&error, "target_disabled");
    // wait_for needs no pipeline and is unchanged by the flag.
    act_on(
        &driver,
        &session,
        "Pick",
        VerifiedTargetOperation::WaitFor { timeout_ms: 100 },
    )
    .await
    .expect("wait_for");
    driver.release(session).await.expect("release");
}

/// Puppeteer behaviour is [UNVERIFIED] here: `Input.dispatchMouseEvent` is
/// documented to wait for the renderer, and an `alert()` in the click handler
/// blocks the renderer until the dialog is answered. This test records what
/// this Chromium does: the click must return, with the dialog pending.
#[tokio::test]
async fn click_opening_an_alert_returns_with_the_dialog_pending() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-alert",
        "<button id=b onclick=\"alert('hello');window.__after=true\">Alert</button>",
    )
    .await;
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(9),
        act_on(&driver, &session, "Alert", VerifiedTargetOperation::Click),
    )
    .await
    .expect("the click did not hang on the dialog")
    .expect("a click that opens an alert is delivered");
    eprintln!("alert click returned after {:?}", started.elapsed());
    let dialogs = driver.dialogs(&session).await.expect("dialogs");
    assert_eq!(dialogs.len(), 1, "{dialogs:?}");
    assert_eq!(dialogs[0].kind, "alert");
    // The dialog is still there for the person or the model to answer.
    driver
        .respond_dialog(&session, &dialogs[0].dialog_id, true, None)
        .await
        .expect("answer the dialog");
    assert!(driver.dialogs(&session).await.expect("dialogs").is_empty());
    assert_eq!(page_json(&driver, &session, "window.__after").await, true);
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn child_frame_targets_fall_back_to_synthetic() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-frame",
        "<button id=b onclick=\"window.__outer={t:event.isTrusted}\">Outer</button>\
         <iframe id=f srcdoc=\"<button onclick='parent.__inner={t:event.isTrusted}'>Inner</button>\"></iframe>",
    )
    .await;
    // The srcdoc frame loads after set_content returns.
    for _ in 0..50 {
        let loaded = page_json(
            &driver,
            &session,
            "(() => { const d = document.getElementById('f').contentDocument; return !!d && d.readyState === 'complete' && !!d.querySelector('button'); })()",
        )
        .await;
        if loaded == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let projection = driver
        .native_page_projection(&session)
        .await
        .expect("native projection");
    let page = driver.current_page(&session).await.expect("current page");
    let root = root_frame_id(&page).await.expect("root frame");
    let outer = projection
        .targets
        .iter()
        .find(|target| target.name.as_deref() == Some("Outer"))
        .cloned()
        .expect("outer button");
    // The page-level accessibility tree stops at the frame boundary, so the
    // child frame's own tree gives the iframe button its binding, the way a
    // frame ref does: bound to the child frame.
    let child = projection
        .frames
        .iter()
        .find(|frame| frame.parent_frame_id.is_some())
        .expect("a child frame")
        .frame_id
        .clone();
    let child_tree = page
        .execute(
            GetFullAxTreeParams::builder()
                .frame_id(FrameId::new(child.clone()))
                .build(),
        )
        .await
        .expect("child frame accessibility tree");
    let child_tree = serde_json::to_value(&child_tree.result).expect("tree json");
    let mut inner = raw_nodes_to_native_targets(&child_tree, AX_PROJECTION_NODE_LIMIT)
        .into_iter()
        .find(|target| target.name.as_deref() == Some("Inner"))
        .expect("the iframe button is a target of its own frame");
    inner.frame_id.get_or_insert(child.clone());
    assert_eq!(inner.frame_id.as_deref(), Some(child.as_str()));
    assert_ne!(child, root, "the button lives in the child frame");

    // The choice itself.
    assert!(matches!(
        driver.input_mode_for(&session, &page, &inner).await,
        InputMode::Synthetic
    ));
    assert!(matches!(
        driver.input_mode_for(&session, &page, &outer).await,
        InputMode::Trusted(_)
    ));
    let mut unframed = outer.clone();
    unframed.frame_id = None;
    assert!(matches!(
        driver.input_mode_for(&session, &page, &unframed).await,
        InputMode::Trusted(_)
    ));
    let mut rooted = outer.clone();
    rooted.frame_id = Some(root);
    assert!(matches!(
        driver.input_mode_for(&session, &page, &rooted).await,
        InputMode::Trusted(_)
    ));

    // And what it does: the iframe click works and is the in-page click.
    driver
        .act_on_verified_target(
            &session,
            target_action(inner, VerifiedTargetOperation::Click),
        )
        .await
        .expect("child-frame click");
    assert_eq!(
        page_json(&driver, &session, "window.__inner.t").await,
        false,
        "a child-frame target keeps the synthetic path"
    );
    driver
        .act_on_verified_target(
            &session,
            target_action(outer, VerifiedTargetOperation::Click),
        )
        .await
        .expect("top-level click");
    assert_eq!(page_json(&driver, &session, "window.__outer.t").await, true);
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn download_targets_still_use_the_synthetic_path() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-download",
        "<a id=d download=\"hi.txt\" href=\"data:text/plain;base64,aGk=\" onclick=\"window.__r={t:event.isTrusted}\">Get</a>",
    )
    .await;
    let target = ref_named(&driver, &session, "Get").await;
    let file = driver
        .download_from_verified_target(&session, target)
        .await
        .expect("governed download");
    assert_eq!(&file.bytes[..], b"hi");
    assert_eq!(
        page_json(&driver, &session, "window.__r.t").await,
        false,
        "the governed download is still triggered by the in-page click"
    );
    driver.release(session).await.expect("release");
}

// ---- Round 6: the branches of the hit test, the other intents, the value route,
// and the bound with a dialog pending.

/// A click whose point lands on a descendant of the target (an icon span in a
/// button) is a click on the target, not a cover.
#[tokio::test]
async fn a_click_landing_on_a_descendant_passes_the_hit_test() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-descendant",
        "<button id=b aria-label=\"Go\" style=\"padding:0\" onclick=\"window.__r={t:event.isTrusted,n:(window.__n=(window.__n||0)+1)}\">\
         <span id=s style=\"display:inline-block;width:120px;height:40px\"></span></button>",
    )
    .await;
    // The pointer at the button's centre is over the span, not the button.
    assert_eq!(
        page_json(
            &driver,
            &session,
            "(() => { const r = document.getElementById('b').getBoundingClientRect();              return document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2).id; })()"
        )
        .await,
        "s"
    );
    act_on(&driver, &session, "Go", VerifiedTargetOperation::Click)
        .await
        .expect("click through the span");
    let seen = page_json(&driver, &session, "window.__r").await;
    assert_eq!(
        (seen["t"].clone(), seen["n"].clone()),
        (true.into(), 1.into())
    );
    driver.release(session).await.expect("release");
}

/// A control covered by its own `<label>` is reached through the label: the
/// label forwards the click, so the hit test accepts it.
#[tokio::test]
async fn a_control_under_its_own_label_passes_the_hit_test() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-label",
        "<input type=checkbox id=c aria-label=\"Agree box\" style=\"position:absolute;left:10px;top:10px;width:20px;height:20px;margin:0\">\
         <label for=c style=\"position:absolute;left:0;top:0;width:200px;height:60px;background:#ddd\">Tick here</label>",
    )
    .await;
    // The label really is what the pointer hits.
    assert_eq!(
        page_json(
            &driver,
            &session,
            "document.elementFromPoint(20, 20).localName"
        )
        .await,
        "label"
    );
    act_on(
        &driver,
        &session,
        "Agree box",
        VerifiedTargetOperation::Click,
    )
    .await
    .expect("click reaches the checkbox through its label");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('c').checked").await,
        true
    );
    // A label that belongs to another control is a cover.
    run_in_page(
        &driver,
        &session,
        "document.querySelector('label').htmlFor = 'nothing';",
    )
    .await;
    let error = act_on(
        &driver,
        &session,
        "Agree box",
        VerifiedTargetOperation::Click,
    )
    .await
    .expect_err("a label for something else covers it");
    assert_repair(&error, "target_covered");
    assert_eq!(
        error.details.as_ref().expect("details")["covered_by_tag"],
        "label"
    );
    driver.release(session).await.expect("release");
}

/// Open and closed shadow roots, a slotted target, and a cover that lives
/// inside an open shadow root.
#[tokio::test]
async fn shadow_roots_and_slots_are_hit_tested_through() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-shadow",
        "<div id=open_host style=\"position:absolute;left:10px;top:10px\"></div>\
         <div id=closed_host style=\"position:absolute;left:10px;top:70px\"></div>\
         <fancy-card id=card style=\"position:absolute;left:10px;top:130px;display:block\">\
           <button id=slotted>Slotted</button></fancy-card>\
         <script>\
           window.__log = [];\
           const click = (name) => (e) => window.__log.push([name, e.isTrusted]);\
           const open = document.getElementById('open_host').attachShadow({mode:'open'});\
           open.innerHTML = '<button id=in_open>Open inner</button>';\
           open.getElementById('in_open').addEventListener('click', click('open'));\
           const closed = document.getElementById('closed_host').attachShadow({mode:'closed'});\
           closed.innerHTML = '<button id=in_closed>Closed inner</button>';\
           closed.getElementById('in_closed').addEventListener('click', click('closed'));\
           const card = document.getElementById('card').attachShadow({mode:'open'});\
           card.innerHTML = '<div style=\"padding:8px\"><slot></slot></div>';\
           document.getElementById('slotted').addEventListener('click', click('slotted'));\
         </script>",
    )
    .await;
    for name in ["Open inner", "Closed inner", "Slotted"] {
        act_on(&driver, &session, name, VerifiedTargetOperation::Click)
            .await
            .unwrap_or_else(|error| panic!("{name} should be clickable: {error:?}"));
    }
    assert_eq!(
        page_json(&driver, &session, "window.__log").await,
        serde_json::json!([["open", true], ["closed", true], ["slotted", true]])
    );

    // A cover inside an open shadow root is found and named by its tag.
    run_in_page(
        &driver,
        &session,
        "const host = document.createElement('div');\
         host.style.cssText = 'position:fixed;left:0;top:0;right:0;bottom:0';\
         document.body.appendChild(host);\
         host.attachShadow({mode:'open'}).innerHTML = \
           '<aside style=\"position:fixed;left:0;top:0;right:0;bottom:0\"></aside>';",
    )
    .await;
    let error = act_on(
        &driver,
        &session,
        "Open inner",
        VerifiedTargetOperation::Click,
    )
    .await
    .expect_err("a cover inside a shadow root");
    assert_repair(&error, "target_covered");
    assert_eq!(
        error.details.as_ref().expect("details")["covered_by_tag"],
        "aside"
    );
    assert_eq!(
        page_json(&driver, &session, "window.__log.length").await,
        3,
        "the covered click did not happen"
    );
    driver.release(session).await.expect("release");
}

/// Typing and selecting go through the same cover and stillness checks as a click.
#[tokio::test]
async fn type_and_select_are_refused_under_an_overlay_and_while_moving() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-type-covered",
        "<input id=i aria-label=\"Name\" value=\"keep\" style=\"position:absolute;left:20px;top:20px\">\
         <select id=s aria-label=\"Pick\" style=\"position:absolute;left:20px;top:80px\">\
           <option value=a>A</option><option value=b>B</option></select>\
         <div id=ov style=\"position:fixed;left:0;top:0;right:0;bottom:0;background:rgba(0,0,0,.3)\"></div>",
    )
    .await;
    let typed = |text: &str| VerifiedTargetOperation::Type { text: text.into() };
    let error = act_on(&driver, &session, "Name", typed("changed"))
        .await
        .expect_err("a field under a modal is not typed into");
    assert_repair(&error, "target_covered");
    assert_eq!(
        error.details.as_ref().expect("details")["covered_by_tag"],
        "div"
    );
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('i').value").await,
        "keep",
        "nothing was typed"
    );
    let error = act_on(
        &driver,
        &session,
        "Pick",
        VerifiedTargetOperation::Select { value: "b".into() },
    )
    .await
    .expect_err("a select under a modal is not set");
    assert_repair(&error, "target_covered");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('s').value").await,
        "a"
    );
    // Dismissed: both work.
    run_in_page(&driver, &session, "document.getElementById('ov').remove()").await;
    act_on(&driver, &session, "Name", typed("changed"))
        .await
        .expect("type after dismissal");
    act_on(
        &driver,
        &session,
        "Pick",
        VerifiedTargetOperation::Select { value: "b".into() },
    )
    .await
    .expect("select after dismissal");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('i').value").await,
        "changed"
    );
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('s').value").await,
        "b"
    );

    // A field that keeps moving is not typed into either.
    run_in_page(
        &driver,
        &session,
        "const style = document.createElement('style');\
         style.textContent = '@keyframes slide{from{transform:translateX(0)}to{transform:translateX(2000px)}}\
           #i{animation:slide 20s linear forwards}';\
         document.head.appendChild(style);",
    )
    .await;
    let error = act_on(&driver, &session, "Name", typed("moving"))
        .await
        .expect_err("a moving field");
    assert_repair(&error, "target_unstable");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('i').value").await,
        "changed"
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
async fn type_replaces_the_text_of_a_contenteditable_element() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-editable",
        "<div id=e role=textbox aria-label=\"Editor\" contenteditable=true style=\"width:200px;height:40px\">old words</div>\
         <script>window.__ev=[];\
           document.getElementById('e').addEventListener('input', e => window.__ev.push([e.type, e.isTrusted]));</script>",
    )
    .await;
    act_on(
        &driver,
        &session,
        "Editor",
        VerifiedTargetOperation::Type {
            text: "new words".into(),
        },
    )
    .await
    .expect("type into contenteditable");
    assert_eq!(
        page_json(
            &driver,
            &session,
            "document.getElementById('e').textContent"
        )
        .await,
        "new words"
    );
    let events = page_json(&driver, &session, "window.__ev").await;
    assert!(
        events
            .as_array()
            .expect("events")
            .iter()
            .any(|event| event[1] == true),
        "{events}"
    );
    driver.release(session).await.expect("release");
}

/// The value route: a date, a range and a colour take a value but not typed
/// text. They go through the same checks and then the in-page setter (untrusted
/// events). A checkbox or a submit button is not a field at all.
#[tokio::test]
async fn typed_values_for_date_and_range_use_the_setter_and_other_inputs_are_refused() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-setvalue",
        "<input id=d type=date>\
         <input id=r type=range min=0 max=10 aria-label=\"Level\">\
         <input id=c type=color>\
         <input id=k type=checkbox aria-label=\"Tick\">\
         <input id=u type=submit value=\"Send\">\
         <script>window.__ev=[];\
           for (const id of ['d','r','c']) { const el = document.getElementById(id);\
             for (const t of ['input','change']) el.addEventListener(t, e => window.__ev.push([id, e.type, e.isTrusted])); }</script>",
    )
    .await;
    let typed = |text: &str| VerifiedTargetOperation::Type { text: text.into() };
    // The accessibility tree shows a date as month, day and year sub-fields and
    // a colour as a well, so those two are addressed by their backend node.
    for (selector, text) in [("#d", "2026-10-21"), ("#c", "#ff8800")] {
        let target = target_of_selector(&driver, &session, selector).await;
        driver
            .act_on_verified_target(&session, target_action(target, typed(text)))
            .await
            .unwrap_or_else(|error| panic!("{selector}: {error:?}"));
    }
    act_on(&driver, &session, "Level", typed("7"))
        .await
        .expect("range");
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('d').value").await,
        "2026-10-21"
    );
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('r').value").await,
        "7"
    );
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('c').value").await,
        "#ff8800"
    );
    let events = page_json(&driver, &session, "window.__ev").await;
    let events = events.as_array().expect("events");
    assert!(events.len() >= 6, "input and change for each: {events:?}");
    assert!(
        events.iter().all(|event| event[2] == false),
        "the setter's events are not trusted: {events:?}"
    );
    // Not a field: refused with the closed reason, and nothing changed.
    for name in ["Tick", "Send"] {
        let error = act_on(&driver, &session, name, typed("x"))
            .await
            .expect_err("not a typeable control");
        assert_repair(&error, "target_disabled");
    }
    assert_eq!(
        page_json(&driver, &session, "document.getElementById('k').checked").await,
        false
    );
    driver.release(session).await.expect("release");
}

/// With the flag on, a JavaScript dialog left pending by one click must not
/// take the bound away from the next ref action: its frame lookup runs under
/// the same `Op::NativeAction` deadline as the action (the CDP calls do not
/// answer while the dialog is open), so it returns within that bound.
#[tokio::test]
async fn a_second_ref_action_with_a_dialog_pending_returns_within_the_bound() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-pending",
        "<button id=a onclick=\"alert('one');window.__after=true\">Alert</button>\
         <button id=o onclick=\"window.__other=true\">Other</button>",
    )
    .await;
    // Both refs are observed first: observing needs the page, which a pending
    // dialog blocks.
    let alert = ref_named(&driver, &session, "Alert").await;
    let mut other = ref_named(&driver, &session, "Other").await;
    // An ordinary top-level target carries no frame id (this Chromium), so the
    // frame lookup would not run; a frame ref does carry its frame's id. Bind the
    // target to the root frame so the lookup that must stay under the deadline is
    // on the path.
    assert!(other.frame_id.is_none());
    let page = driver.current_page(&session).await.expect("current page");
    other.frame_id = Some(root_frame_id(&page).await.expect("root frame"));
    driver
        .act_on_verified_target(
            &session,
            target_action(alert, VerifiedTargetOperation::Click),
        )
        .await
        .expect("the first click opens the alert");
    assert_eq!(driver.dialogs(&session).await.expect("dialogs").len(), 1);

    let bound = crate::deadlines::deadline(crate::deadlines::Op::NativeAction);
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        bound + Duration::from_secs(5),
        driver.act_on_verified_target(
            &session,
            target_action(other, VerifiedTargetOperation::Click),
        ),
    )
    .await
    .expect("the second action returned: it did not wait for the 40 s CDP request timeout");
    let waited = started.elapsed();
    eprintln!(
        "second action with a dialog pending: {waited:?}, {:?}",
        outcome.as_ref().err().map(|e| e.code)
    );
    assert!(waited < bound + Duration::from_secs(3), "{waited:?}");

    // Answer the dialog; the page is usable again.
    let dialogs = driver.dialogs(&session).await.expect("dialogs");
    driver
        .respond_dialog(&session, &dialogs[0].dialog_id, true, None)
        .await
        .expect("answer the dialog");
    driver.release(session).await.expect("release");
}

// Round 7: the three branches of the hit test and of the "gone" answers that no
// fixture reached before.

/// The target is the element in a shadow root that wraps a `<slot>`, and what
/// the pointer lands on is the slotted light-DOM child. Only the
/// `assignedSlot` step of the ancestor climb connects the two: the child's
/// `parentNode` is the shadow host, and the host's ancestors never reach the
/// wrapper in the root.
#[tokio::test]
async fn a_slotted_child_of_the_target_passes_through_the_assigned_slot() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-slot-climb",
        "<fancy-card id=card style=\"position:absolute;left:10px;top:10px;display:block\">\
           <span style=\"display:block;width:140px;height:30px\">light child</span>\
         </fancy-card>\
         <script>\
           window.__log = [];\
           const root = document.getElementById('card').attachShadow({mode:'open'});\
           root.innerHTML = '<div id=wrap role=button tabindex=0 aria-label=\"Slot wrapper\">\
             <slot></slot></div>';\
           root.getElementById('wrap').addEventListener('click', (e) => \
             window.__log.push(['wrap', e.isTrusted, e.target.localName]));\
         </script>",
    )
    .await;
    // What the page says the hit is: the light child, not the wrapper.
    let hit = page_json(
        &driver,
        &session,
        "(() => { const r = document.getElementById('card').firstElementChild\
           .getBoundingClientRect();\
           const h = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);\
           const wrap = document.getElementById('card').shadowRoot.getElementById('wrap');\
           return { hit: h.localName, assigned: !!h.assignedSlot, \
                    wrapper_is_ancestor_by_parent_node: wrap.contains(h) }; })()",
    )
    .await;
    assert_eq!(hit["hit"], "span", "{hit}");
    assert_eq!(hit["assigned"], true, "{hit}");
    assert_eq!(
        hit["wrapper_is_ancestor_by_parent_node"], false,
        "the wrapper is not a DOM ancestor of the slotted child: {hit}"
    );
    act_on(
        &driver,
        &session,
        "Slot wrapper",
        VerifiedTargetOperation::Click,
    )
    .await
    .expect("a slotted child under the pointer is the target's own content");
    assert_eq!(
        page_json(&driver, &session, "window.__log").await,
        serde_json::json!([["wrap", true, "span"]])
    );
    driver.release(session).await.expect("release");
}

/// A known limit, pinned so a change is noticed: a closed shadow root hides
/// what is inside it, so a cover that lives in the same closed root as the
/// target cannot be told from the target. The pipeline accepts it (the hit is
/// the root's host, the target's own component) and the click then lands on the
/// cover. The docs say so (RUNBOOKS, "Trusted input for ref actions"). If
/// detecting this is ever built, this test changes with it.
#[tokio::test]
async fn a_cover_in_the_same_closed_root_as_the_target_is_not_detected() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-closed-cover",
        "<div id=host style=\"position:absolute;left:10px;top:10px\"></div>\
         <script>\
           window.__log = [];\
           const root = document.getElementById('host').attachShadow({mode:'closed'});\
           root.innerHTML = '<button id=under>Under</button>\
             <div id=cover style=\"position:fixed;left:0;top:0;right:0;bottom:0\"></div>';\
           const note = (name) => (e) => window.__log.push([name, e.isTrusted]);\
           root.getElementById('under').addEventListener('click', note('under'));\
           root.getElementById('cover').addEventListener('click', note('cover'));\
         </script>",
    )
    .await;
    act_on(&driver, &session, "Under", VerifiedTargetOperation::Click)
        .await
        .expect("the limit: a cover inside the target's own closed root is not seen");
    assert_eq!(
        page_json(&driver, &session, "window.__log").await,
        serde_json::json!([["cover", true]]),
        "the click landed on the cover, not on the target"
    );
    driver.release(session).await.expect("release");
}

/// The node leaves the document after it was resolved and before the first
/// probe: the answer is `target_not_attached`, at once, with the reason only.
#[tokio::test]
async fn a_node_that_leaves_between_resolve_and_probe_is_not_attached() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-race",
        "<button id=b onclick=\"window.__n=1\">Racing</button>",
    )
    .await;
    let target = ref_named(&driver, &session, "Racing").await;
    let page = driver.current_page(&session).await.expect("current page");
    let resolved = page
        .execute(
            ResolveNodeParams::builder()
                .backend_node_id(BackendNodeId::new(target.backend_node_id))
                .build(),
        )
        .await
        .expect("resolve");
    let object_id = resolved.object.object_id.clone().expect("object id");
    // Resolved. Now the page removes it.
    run_in_page(&driver, &session, "document.getElementById('b').remove();").await;
    let started = Instant::now();
    let error = crate::actionability::prepare_click(&page, &object_id)
        .await
        .expect_err("a removed node is not clicked");
    assert_repair(&error, "target_not_attached");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "a gone node is final at once, not after the 2 s budget: {:?}",
        started.elapsed()
    );
    assert_eq!(
        page_json(&driver, &session, "window.__n").await,
        serde_json::Value::Null
    );
    driver.release(session).await.expect("release");
}

/// The document the node belonged to is replaced by a navigation: the realm the
/// handle lived in is destroyed, and the probe's own call fails. That is the
/// "gone" answer too (`node_is_gone`), not an internal error.
#[tokio::test]
async fn a_handle_whose_realm_was_destroyed_by_a_navigation_is_not_attached() {
    if !live_enabled() {
        return;
    }
    let (driver, session) = trusted_fixture(
        true,
        "ti-navigated",
        "<button id=b onclick=\"window.__n=1\">Doomed</button>",
    )
    .await;
    let target = ref_named(&driver, &session, "Doomed").await;
    let page = driver.current_page(&session).await.expect("current page");
    let resolved = page
        .execute(
            ResolveNodeParams::builder()
                .backend_node_id(BackendNodeId::new(target.backend_node_id))
                .build(),
        )
        .await
        .expect("resolve");
    let object_id = resolved.object.object_id.clone().expect("object id");
    page.goto("about:blank").await.expect("navigate away");
    let error = crate::actionability::prepare_click(&page, &object_id)
        .await
        .expect_err("a handle from a destroyed realm is not clicked");
    eprintln!("destroyed realm: {error:?}");
    assert_repair(&error, "target_not_attached");

    // And the other end of the same story: the ref itself. After the navigation
    // `DOM.resolveNode` cannot resolve the old backend node, and the answer is
    // the older reason-less repair error (no `details.reason` from the closed
    // list), which ERROR_REASONS describes.
    let error = driver
        .act_on_verified_target(
            &session,
            target_action(target, VerifiedTargetOperation::Click),
        )
        .await
        .expect_err("a ref from the previous document");
    eprintln!("stale ref after navigation: {error:?}");
    assert_eq!(error.code, ErrorCode::TargetRepairRequired, "{error:?}");
    assert_eq!(error.target_repair_reason(), None, "{error:?}");
    driver.release(session).await.expect("release");
}
