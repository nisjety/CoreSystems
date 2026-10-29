//! Property tests (WS4) for the WS2 DOM-for-LLM scanner.
//!
//! Invariants held over arbitrary generated HTML:
//! 1. Every surviving interactive element gets a dense, gap-free,
//!    1-based `click_index` aligned with the `@eN` click map rows
//!    (`click_map[i].index == i + 1`, `ref_id == @e{index}`).
//! 2. Elements explicitly marked invisible are never present; a page
//!    with only invisible controls yields no click map.
//! 3. Wire-shape stability: `DomSummary` round-trips through JSON and
//!    omits `click_map` when there is nothing to click.
//!
//! The scanner is intentionally attribute-level (no layout engine), so
//! generation stays within its documented input domain: well-formed
//! opening tags for the five scanned element kinds plus optional
//! invisibility markers.

use proptest::prelude::*;
use quarry_core::contracts::DomSummary;

fn arb_tag() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("a"),
        Just("button"),
        Just("input"),
        Just("select"),
        Just("textarea"),
    ]
}

/// A single generated control: optional id/testid/name/aria-label plus
/// an optional invisibility marker from the set the scanner honours.
fn arb_control(idx: usize) -> impl Strategy<Value = String> {
    (
        arb_tag(),
        prop::option::of("[a-z][a-z0-9-]{0,12}"),
        prop::option::of("[A-Za-z ]{0,20}"),
        prop::option::of(prop_oneof![
            Just("hidden"),
            Just("aria-hidden=\"true\""),
            Just("style=\"display:none\""),
            Just("style=\"visibility:hidden\""),
            Just("style=\"opacity:0\"")
        ]),
    )
        .prop_map(move |(tag, id, label, hide)| {
            let mut attrs = String::new();
            if let Some(id) = id {
                attrs.push_str(&format!(" id=\"{id}-{idx}\""));
            }
            if let Some(label) = label {
                attrs.push_str(&format!(" aria-label=\"{label}\""));
            }
            if let Some(hide) = hide {
                attrs.push(' ');
                attrs.push_str(hide);
            }
            format!("<{tag}{attrs}>Label {idx}</{tag}>")
        })
}

fn arb_page() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<usize>().prop_flat_map(arb_control), 0..24).prop_map(
        |controls| {
            let body: String = controls.join("\n");
            format!("<html><body>{body}</body></html>")
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn click_indices_are_dense_one_based_and_aligned(page in arb_page()) {
        let summary = quarry_runtime::observation::build_dom_summary(&page);
        if summary.interactive_elements.is_empty() {
            prop_assert!(summary.click_map.is_none());
            return Ok(());
        }
        let click_map = summary.click_map.as_ref().expect("non-empty page has a click map");
        prop_assert_eq!(click_map.len(), summary.interactive_elements.len());
        // Dense 1-based numbering over exactly the survivors, aligned
        // with the snapshot-ref convention.
        for (offset, row) in click_map.iter().enumerate() {
            let expected = offset as u32 + 1;
            prop_assert_eq!(row.index, expected);
            prop_assert_eq!(&row.ref_id, &format!("@e{expected}"));
            prop_assert_eq!(
                summary.interactive_elements[offset].click_index,
                Some(expected),
                "element {} must carry click_index {}",
                offset,
                expected
            );
            prop_assert_eq!(&row.tag, &summary.interactive_elements[offset].tag);
        }
    }

    #[test]
    fn invisible_controls_never_survive(
        tag in arb_tag(),
        label_id in "[a-z][a-z0-9-]{0,10}",
        hide in prop_oneof![
            Just(" hidden"),
            Just(" aria-hidden=\"true\""),
            Just(" style=\"display:none\""),
            Just(" style=\"visibility:hidden\""),
            Just(" style=\"opacity:0\""),
            Just(" type=\"hidden\""),
        ],
    ) {
        // A page whose ONLY control is invisible must produce no
        // interactive elements and no click map. (The `type="hidden"`
        // marker only applies to inputs; other tags ignore it.)
        let html = format!(
            "<html><body><p>content</p><{tag} id=\"{label_id}\"{hide}>x</{tag}></body></html>",
            tag = tag
        );
        let summary = quarry_runtime::observation::build_dom_summary(&html);
        let relevant = !(hide == " type=\"hidden\"" && tag != "input");
        if !relevant {
            return Ok(());
        }
        prop_assert!(
            summary.interactive_elements.is_empty(),
            "hidden <{}> survived filtering",
            tag
        );
        prop_assert!(summary.click_map.is_none());
    }

    #[test]
    fn visible_controls_always_survive_with_a_dense_map(
        count in 1usize..24,
        tag in arb_tag(),
    ) {
        let mut body = String::new();
        for i in 0..count {
            body.push_str(&format!("<{tag} id=\"ctl-{i}\">Go</{tag}>"));
        }
        let html = format!("<html><body>{body}</body></html>");
        let summary = quarry_runtime::observation::build_dom_summary(&html);
        prop_assert_eq!(summary.interactive_elements.len() as u64, count as u64);
        let click_map = summary.click_map.expect("visible controls yield a map");
        prop_assert_eq!(click_map.len(), count);
        prop_assert_eq!(click_map.last().unwrap().index, count as u32);
    }

    #[test]
    fn dom_summary_roundtrips_through_json(page in arb_page()) {
        let summary = quarry_runtime::observation::build_dom_summary(&page);
        let json = serde_json::to_value(&summary).unwrap();
        if summary.click_map.is_none() {
            prop_assert!(!json.as_object().unwrap().contains_key("click_map"));
        }
        let back: DomSummary = serde_json::from_value(json).unwrap();
        prop_assert_eq!(back.node_count, summary.node_count);
        prop_assert_eq!(back.interactive_elements.len(), summary.interactive_elements.len());
        prop_assert_eq!(back.click_map.is_some(), summary.click_map.is_some());
    }
}
