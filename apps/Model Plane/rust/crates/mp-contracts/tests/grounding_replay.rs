//! Replays the argument fabrications actually observed on 2026-08-25 through the
//! grounding gate.
//!
//! Unit tests prove the rule; this proves the rule against the real outputs that
//! motivated it. Every `args` below is a verbatim tool call the live model
//! emitted, and every one was schema-valid — which is why the schema validator
//! let all of them through.

use mp_contracts::tool_arguments::ungrounded_arguments;

/// (conversation, arguments, must_be_refused)
const OBSERVED: &[(&str, &str, bool)] = &[
    // --- fabrications: the model invented postal codes and dimensions ---
    (
        "hva vil det koste å sende 3 kg fra Bergen til Stavanger?",
        r#"{"from":{"name":"Bergen","postal_code":"5000","city":"Bergen","country":"NO"},
            "to":{"name":"Stavanger","postal_code":"4000","city":"Stavanger","country":"NO"},
            "weight_kg":3,"length_cm":30,"width_cm":20,"height_cm":10,"segment":"b2c"}"#,
        true,
    ),
    (
        "sammenlign fraktpriser for en 12 kg pall fra Oslo til Tromsø",
        r#"{"from":{"name":"Oslo","postal_code":"0010","city":"Oslo","country":"NO","is_business":true},
            "to":{"name":"Tromsø","postal_code":"9000","city":"Tromsø","country":"NO","is_business":true},
            "weight_kg":12,"length_cm":120,"width_cm":80,"height_cm":100,"segment":"b2b"}"#,
        true,
    ),
    (
        "what would it cost to send 3 kg from Bergen to Stavanger?",
        r#"{"from":{"name":"Sender","city":"Bergen","country":"NO","postal_code":"5000"},
            "to":{"name":"Recipient","city":"Stavanger","country":"NO","postal_code":"4000"},
            "weight_kg":3,"length_cm":30,"width_cm":20,"height_cm":10,"segment":"b2c"}"#,
        true,
    ),
    (
        // the residual observed AFTER the prompt fix — still fabricated, and the
        // invented sender name came from the preamble, not the request
        "hva vil det koste å sende 3 kg fra Bergen til Stavanger?",
        r#"{"from":{"name":"Verevon","postal_code":"5003","city":"Bergen"},
            "to":{"name":"Verevon","postal_code":"4001","city":"Stavanger"},
            "weight_kg":3,"segment":"b2c"}"#,
        true,
    ),
    // --- legitimate: everything stated, so nothing may be refused ---
    (
        "compare shipping prices for a 5 kg parcel, 30x20x15 cm, from Storgata 1, \
         0155 Oslo to Kongens gate 2, 7011 Trondheim",
        r#"{"from":{"name":"Storgata 1","postal_code":"0155","city":"Oslo"},
            "to":{"name":"Kongens gate 2","postal_code":"7011","city":"Trondheim"},
            "weight_kg":5,"length_cm":30,"width_cm":20,"height_cm":15,"segment":"b2b"}"#,
        false,
    ),
];

#[test]
fn every_observed_fabrication_is_refused_and_no_legitimate_call_is() {
    let mut refused = 0;
    let mut allowed = 0;
    for (conversation, args, must_refuse) in OBSERVED {
        let errors = ungrounded_arguments("get_shipping_quotes", args, conversation);
        if *must_refuse {
            assert!(
                !errors.is_empty(),
                "this call invented values and was NOT refused:\n  {args}\n  against: {conversation}"
            );
            refused += 1;
        } else {
            assert!(
                errors.is_empty(),
                "this call stated every value and WAS refused on {:?} — a false refusal is \
                 worse than no check",
                errors.iter().map(|e| e.field.as_str()).collect::<Vec<_>>()
            );
            allowed += 1;
        }
    }
    assert_eq!((refused, allowed), (4, 1), "the corpus changed shape");
}
