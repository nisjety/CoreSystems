p='agent.work.rs'
s=open(p,encoding='utf-8',newline='').read()
def r(a,b):
    global s
    assert s.count(a)==1,a
    s=s.replace(a,b)
r("""                    // dispatched and the model reads that the person is using the browser.
                    let still_taken_over = match browser.as_ref() {
                        Some(browser)
                            if is_release
                                && crate::browser_session::is_do_tool(&call.name)
                                && crate::browser_session::takeover_pause_enabled() =>
                        {
                            released_browser_call_still_held(ctx, browser).await
                        }
                        _ => false,
                    };
""","""                    // dispatched and the model reads that the person is using the browser. EP-1 S5:
                    // after the give-back it also needs a grant that outlived the takeover, read back
                    // under the person's click-time bearer; a run whose wait ran out is told so.
                    let handback_refusal = if is_release
                        && crate::browser_session::is_do_tool(&call.name)
                        && crate::browser_session::takeover_pause_enabled()
                    {
                        match browser.as_ref() {
                            Some(browser) => released_browser_call_refusal(ctx, browser).await,
                            None if !zdr && ctx.depth == 0 && req.space_id.is_empty() => {
                                crate::browser_session::handback_without_bearer(
                                    ctx.state,
                                    &req.run_id,
                                )
                            }
                            None => None,
                        }
                    } else {
                        None
                    };
""")
r("""                                && !still_taken_over
""","""                                && handback_refusal.is_none()
""")
r("""                    let outcome = if still_taken_over {
                        Some(StepOutcome::failed(
                            &crate::browser_session::TAKEN_OVER.text(),
                        ))
                    } else if let Some((look, _)) = hand_back""","""                    let outcome = if let Some(refusal) = handback_refusal {
                        Some(StepOutcome::failed(&refusal.text()))
                    } else if let Some((look, _)) = hand_back""")
open(p,'w',encoding='utf-8',newline='').write(s)
