//! E3.3-h: the read-only point hit test. `GET /v1/agent/runs/{run_id}/hit-test?x=&y=` maps a
//! viewport point (CSS pixels from the top-left corner) to the observed `@e` ref under it.
//!
//! This is a Quarry route, not an `AgentAction` (R-12). It executes nothing, navigates nowhere,
//! focuses or scrolls nothing and writes nothing. It is dark until `QUARRY_EDGE__HIT_TEST` is on,
//! and that switch is off by default. A ZDR run may use it: the answer goes back to the caller
//! and is neither stored nor logged, and the route logs nothing about the point or the page.
//!
//! The browser half is `ChromiumoxideDriver::hit_test_point` (`DOM.getNodeForLocation`, then the
//! node's accessibility ancestors). The ref half reads the person's lane, the last observation the
//! caller already holds, and returns the nearest ancestor that is one of its targets. Backend node
//! ids never leave this process.
//!
//! Not wired yet: `BrowserDriver` has no point hit test. The trait lives in
//! `quarry-browser/src/lib.rs`, which this unit does not own, so [`point_hit`] answers
//! `unsupported` until that default method lands (see the unit's status file).

use super::*;

use quarry_browser::chromiumoxide::PointHit;

/// The switch. Read per call, like `QUARRY_EDGE__DOWNLOADS`, so it needs no AppState plumbing.
pub const HIT_TEST_FLAG: &str = "QUARRY_EDGE__HIT_TEST";

/// The words that turn the switch on, read the way the other switches read theirs.
pub(super) fn parse_hit_test_flag(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn hit_test_enabled() -> bool {
    parse_hit_test_flag(std::env::var(HIT_TEST_FLAG).ok().as_deref())
}

/// The point as the caller sends it: CSS pixels from the viewport's top-left corner.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct HitTestQuery {
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// Whole CSS pixels inside the viewport, which is what Chromium's hit test takes. A point outside
/// the viewport is refused here, before the browser is asked.
pub(super) fn point_from_query(
    query: HitTestQuery,
    viewport: BrowserViewport,
) -> Result<(i64, i64), &'static str> {
    let (Some(x), Some(y)) = (query.x, query.y) else {
        return Err("x and y are required");
    };
    if !x.is_finite() || !y.is_finite() {
        return Err("x and y must be finite numbers");
    }
    let (x, y) = (x.round(), y.round());
    if x < 0.0 || y < 0.0 || x >= f64::from(viewport.width) || y >= f64::from(viewport.height) {
        return Err("x and y must lie inside the viewport");
    }
    // In range, so the casts cannot wrap.
    Ok((x as i64, y as i64))
}

/// One observed target of the caller's snapshot, with the DOM node it is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedRef {
    pub backend_node_id: i64,
    pub target_ref: String,
    pub role: Option<String>,
    pub name: Option<String>,
}

/// The snapshot of a lane's last observation, as the hit test reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedSnapshot {
    pub snapshot_id: String,
    pub generation: u32,
    pub targets: Vec<ObservedRef>,
}

/// The DOM node a Chromium-native target is bound to. quarry-runtime writes the fingerprint of a
/// native target as `ax:{ax_node_id}:{backend_node_id}` (`native_target_fingerprint`). A target
/// with any other shape is not bound to a DOM node, and it is skipped.
pub(super) fn backend_from_fingerprint(fingerprint_id: &str) -> Option<i64> {
    let rest = fingerprint_id.strip_prefix("ax:")?;
    let (_, backend) = rest.rsplit_once(':')?;
    backend.parse().ok()
}

/// The snapshot of the lane's last observation, or `None` when the lane has not observed a page.
/// Takes the lane's own lock only, and only after the run entry lock is released.
pub(super) async fn observed_snapshot(lane: &LaneRef) -> Option<ObservedSnapshot> {
    let observation = lane.0.lock().await.last_observation.clone()?;
    let snapshot = observation.snapshot.as_ref()?;
    Some(ObservedSnapshot {
        snapshot_id: snapshot.snapshot_id.clone(),
        generation: snapshot.generation,
        targets: snapshot
            .targets
            .iter()
            .filter_map(|target| {
                let backend_node_id =
                    backend_from_fingerprint(&target.fingerprint.as_ref()?.fingerprint_id)?;
                Some(ObservedRef {
                    backend_node_id,
                    target_ref: target.ref_id.clone(),
                    role: target.role.clone(),
                    name: target.name.clone(),
                })
            })
            .collect(),
    })
}

/// The observed target bound to the nearest DOM node on `chain` (the node under the point first,
/// then its accessibility ancestors). `None` when none of them is an observed target.
pub(super) fn nearest_observed<'a>(
    chain: &[i64],
    observed: &'a ObservedSnapshot,
) -> Option<&'a ObservedRef> {
    chain.iter().find_map(|backend| {
        observed
            .targets
            .iter()
            .find(|target| target.backend_node_id == *backend)
    })
}

/// The answer to one point. `hit` says whether Chromium found a node under the point. `ref` is set
/// only when that node, or one of its accessibility ancestors, is a target of the caller's observed
/// snapshot; `snapshotId` and `generation` name that snapshot, so the caller can tell it is stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitTestData {
    pub hit: bool,
    #[serde(rename = "ref")]
    pub target_ref: Option<String>,
    pub snapshot_id: Option<String>,
    pub generation: Option<u32>,
    pub role: Option<String>,
    pub name: Option<String>,
    /// Why there is no `ref`: `no_node`, `no_snapshot` or `not_observed`.
    pub reason: Option<&'static str>,
    pub zdr: bool,
}

/// The answer for one point, from what the browser found and what the lane last observed.
pub(super) fn hit_answer(
    hit: Option<&PointHit>,
    observed: Option<&ObservedSnapshot>,
    zdr: bool,
) -> HitTestData {
    let without_ref = |found: bool, reason: &'static str| HitTestData {
        hit: found,
        target_ref: None,
        snapshot_id: None,
        generation: None,
        role: None,
        name: None,
        reason: Some(reason),
        zdr,
    };
    let Some(hit) = hit else {
        return without_ref(false, "no_node");
    };
    let Some(observed) = observed else {
        return without_ref(true, "no_snapshot");
    };
    match nearest_observed(&hit.backend_node_ids, observed) {
        Some(target) => HitTestData {
            hit: true,
            target_ref: Some(target.target_ref.clone()),
            snapshot_id: Some(observed.snapshot_id.clone()),
            generation: Some(observed.generation),
            role: target.role.clone(),
            name: target.name.clone(),
            reason: None,
            zdr,
        },
        None => without_ref(true, "not_observed"),
    }
}

/// The browser half: the DOM node under the point and its accessibility ancestors. `BrowserDriver`
/// has no point hit test yet. The trait lives in `quarry-browser/src/lib.rs`, which this unit does
/// not own, so every call answers `unsupported` until that default method lands. It will delegate
/// to `ChromiumoxideDriver::hit_test_point`.
async fn point_hit(
    state: &AppState,
    session: &BrowserSession,
    x: i64,
    y: i64,
) -> QuarryResult<Option<PointHit>> {
    let _ = (state, session, x, y);
    Err(QuarryError::unsupported_action("hit_test"))
}

/// `GET /v1/agent/runs/{run_id}/hit-test?x=&y=` (E3.3-h). Answers `403 hit_test_disabled` until
/// `QUARRY_EDGE__HIT_TEST` is on, `404` for a run this caller does not own, and `400` for a point
/// outside the viewport. It touches nothing but the live-run liveness that the other read routes
/// touch.
pub async fn hit_test_point(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<HitTestQuery>,
) -> Result<Json<Envelope<HitTestData>>, ApiErr> {
    let request_id = crate::request_id::current();
    if !hit_test_enabled() {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Forbidden, "hit_test_disabled"),
        ));
    }
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    let Some(entry_arc) = entry_arc else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    // The entry lock is released before the browser is asked and before the lane is read: a lane
    // is taken before an entry, never the other way round.
    let (session, lane, zdr, viewport) = {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        (
            entry.session.clone(),
            entry.human_lane.clone(),
            entry.zdr.is_active(),
            entry.lease.viewport.unwrap_or_else(default_stream_viewport),
        )
    };
    let (x, y) = point_from_query(query, viewport)
        .map_err(|message| status_err(StatusCode::BAD_REQUEST, &request_id, message))?;
    let hit = point_hit(&state, &session, x, y)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let observed = observed_snapshot(&lane).await;
    Ok(Json(Envelope::ok(
        request_id,
        hit_answer(hit.as_ref(), observed.as_ref(), zdr),
    )))
}

#[cfg(test)]
mod hit_test_tests {
    use super::*;

    fn viewport() -> BrowserViewport {
        BrowserViewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        }
    }

    fn target(backend_node_id: i64, target_ref: &str, name: &str) -> ObservedRef {
        ObservedRef {
            backend_node_id,
            target_ref: target_ref.to_owned(),
            role: Some("button".to_owned()),
            name: Some(name.to_owned()),
        }
    }

    fn observed(targets: Vec<ObservedRef>) -> ObservedSnapshot {
        ObservedSnapshot {
            snapshot_id: "snap_1".to_owned(),
            generation: 3,
            targets,
        }
    }

    #[test]
    fn the_switch_is_on_only_for_the_usual_words() {
        assert!(!parse_hit_test_flag(None));
        assert!(!parse_hit_test_flag(Some("")));
        assert!(!parse_hit_test_flag(Some("0")));
        assert!(!parse_hit_test_flag(Some("off")));
        assert!(!parse_hit_test_flag(Some("maybe")));
        assert!(parse_hit_test_flag(Some("1")));
        assert!(parse_hit_test_flag(Some(" TRUE ")));
        assert!(parse_hit_test_flag(Some("on")));
        assert!(parse_hit_test_flag(Some("yes")));
    }

    #[test]
    fn a_point_is_whole_pixels_inside_the_viewport() {
        let at = |x: f64, y: f64| HitTestQuery {
            x: Some(x),
            y: Some(y),
        };
        assert_eq!(point_from_query(at(10.4, 20.6), viewport()), Ok((10, 21)));
        assert_eq!(point_from_query(at(0.0, 0.0), viewport()), Ok((0, 0)));
        assert_eq!(
            point_from_query(at(1279.0, 799.0), viewport()),
            Ok((1279, 799))
        );
        assert!(point_from_query(at(1280.0, 0.0), viewport()).is_err());
        assert!(point_from_query(at(0.0, 800.0), viewport()).is_err());
        assert!(point_from_query(at(-1.0, 0.0), viewport()).is_err());
        assert!(point_from_query(at(f64::NAN, 0.0), viewport()).is_err());
        assert!(point_from_query(at(0.0, f64::INFINITY), viewport()).is_err());
        let missing = HitTestQuery {
            x: None,
            y: Some(1.0),
        };
        assert!(point_from_query(missing, viewport()).is_err());
    }

    #[test]
    fn a_fingerprint_names_its_backend_node_only_in_the_native_shape() {
        assert_eq!(backend_from_fingerprint("ax:17:4242"), Some(4242));
        assert_eq!(backend_from_fingerprint("ax:node:4242"), Some(4242));
        assert_eq!(backend_from_fingerprint("css:div:4242"), None);
        assert_eq!(backend_from_fingerprint("ax:17"), None);
        assert_eq!(backend_from_fingerprint("ax:17:not-a-number"), None);
    }

    #[test]
    fn the_nearest_observed_ancestor_wins() {
        let snapshot = observed(vec![target(30, "@e2", "Card"), target(10, "@e1", "Save")]);
        // The node under the point is a text span; its button is the nearest observed target.
        let chain: [i64; 3] = [20, 10, 30];
        let found = nearest_observed(&chain, &snapshot).expect("a button on the chain");
        assert_eq!(found.target_ref, "@e1");
        let outer: [i64; 2] = [99, 30];
        assert_eq!(
            nearest_observed(&outer, &snapshot).map(|t| t.target_ref.as_str()),
            Some("@e2")
        );
        let unrelated: [i64; 1] = [99];
        assert!(nearest_observed(&unrelated, &snapshot).is_none());
    }

    #[test]
    fn the_answer_names_its_snapshot_and_says_why_it_has_no_ref() {
        let snapshot = observed(vec![target(10, "@e1", "Save")]);
        let hit = PointHit {
            backend_node_ids: vec![10],
            frame_id: Some("frame-1".to_owned()),
        };
        let found = hit_answer(Some(&hit), Some(&snapshot), false);
        assert!(found.hit);
        assert_eq!(found.target_ref.as_deref(), Some("@e1"));
        assert_eq!(found.snapshot_id.as_deref(), Some("snap_1"));
        assert_eq!(found.generation, Some(3));
        assert_eq!(found.name.as_deref(), Some("Save"));
        assert_eq!(found.reason, None);

        let nothing = hit_answer(None, Some(&snapshot), true);
        assert!(!nothing.hit);
        assert_eq!(nothing.reason, Some("no_node"));
        assert!(nothing.zdr);

        let unobserved = hit_answer(Some(&hit), None, false);
        assert!(unobserved.hit);
        assert_eq!(unobserved.reason, Some("no_snapshot"));

        let elsewhere = PointHit {
            backend_node_ids: vec![55],
            frame_id: None,
        };
        let off = hit_answer(Some(&elsewhere), Some(&snapshot), false);
        assert!(off.hit);
        assert_eq!(off.target_ref, None);
        assert_eq!(off.reason, Some("not_observed"));
    }

    #[test]
    fn the_wire_answer_carries_refs_and_no_browser_handles() {
        let snapshot = observed(vec![target(10, "@e1", "Save")]);
        let hit = PointHit {
            backend_node_ids: vec![10],
            frame_id: Some("frame-1".to_owned()),
        };
        let wire = serde_json::to_value(hit_answer(Some(&hit), Some(&snapshot), false))
            .expect("the answer serialises");
        assert_eq!(wire["hit"], true);
        assert_eq!(wire["ref"], "@e1");
        assert_eq!(wire["snapshotId"], "snap_1");
        assert_eq!(wire["generation"], 3);
        assert!(wire.get("backendNodeIds").is_none());
        assert!(wire.get("frameId").is_none());
    }
}
