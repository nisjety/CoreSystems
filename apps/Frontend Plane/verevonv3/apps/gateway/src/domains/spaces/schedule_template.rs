//! The digest Control commits to when a schedule is created, and the shape a
//! schedule template needs for that digest to survive until the schedule fires.
//!
//! The commitment is checked three times, by three implementations, over the
//! same template:
//!
//! 1. Capability Core, at create, hashes Go's `json.Marshal` of the decoded
//!    template (`CronHandler.listOrCreate`) and verifies Control's decision
//!    against it.
//! 2. The dispatcher, at fire time, hashes the template it reads back out of
//!    Postgres JSONB (`taskexec.canonicalScheduledTemplate`).
//! 3. Orchestrator Core hashes it again before it accepts the run input
//!    (`orchestration.scheduledTemplateGoalAndPolicy`).
//!
//! The BFF asks Control for the decision, so the digest it sends has to be the
//! one Go computes. Two things make the naive `serde_json::to_vec` digest wrong:
//! Go escapes characters serde does not (see [`go_compatible_json_bytes`]), and
//! Postgres JSONB re-orders nested object keys, which Go then re-emits in the
//! stored order (see [`check_digest_stable`]). Every already stored Go digest
//! stays valid because only this side changes.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Largest magnitude a JavaScript client, and Go's `float64` decode of the
/// create body, both represent exactly.
const MAX_SAFE_INTEGER: u64 = 1 << 53;

/// The dispatcher drops this key before hashing: Capability Core's sweeper
/// writes the fire intent under it after the template is read back. A template
/// that carried its own copy would be hashed with it at create and without it
/// at fire.
const RESERVED_FIRE_INTENT_KEY: &str = "schedule_fire_intent";

/// Serialises `value` to the bytes Go's `json.Marshal` produces for the same
/// decoded value.
///
/// Both emit compact JSON with object keys in bytewise order (serde_json's
/// `Map` is a `BTreeMap` here; `serde_json_object_keys_iterate_in_sorted_order`
/// fails if a feature flag ever changes that). They differ only in string
/// escaping: Go also escapes `<`, `>` and `&` as `\u003c`, `\u003e`,
/// `\u0026`, and U+2028 and U+2029 as `\u2028`, `\u2029`. Control
/// characters, `"` and `\` are escaped identically by both (Go 1.22 and later
/// write `\b` and `\f` like serde_json does; the module requires Go 1.25).
///
/// Escaping the serialised bytes is exact because in serde_json's output those
/// characters can only occur inside string literals (the structural characters
/// are `{ } [ ] : , "` and number text), keys included. U+2028 and U+2029 are
/// the UTF-8 sequences E2 80 A8 and E2 80 A9, and E2 only ever starts a
/// three-byte sequence, so the match cannot straddle two characters.
pub(super) fn go_compatible_json_bytes(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let plain = serde_json::to_vec(value)?;
    let mut escaped = Vec::with_capacity(plain.len() + 16);
    let mut index = 0;
    while index < plain.len() {
        match plain[index] {
            b'<' => escaped.extend_from_slice(br"\u003c"),
            b'>' => escaped.extend_from_slice(br"\u003e"),
            b'&' => escaped.extend_from_slice(br"\u0026"),
            0xE2 if plain.get(index + 1) == Some(&0x80)
                && matches!(plain.get(index + 2), Some(0xA8 | 0xA9)) =>
            {
                escaped.extend_from_slice(if plain[index + 2] == 0xA8 {
                    br"\u2028"
                } else {
                    br"\u2029"
                });
                index += 3;
                continue;
            }
            byte => escaped.push(byte),
        }
        index += 1;
    }
    Ok(escaped)
}

/// `sha256:<hex>` over [`go_compatible_json_bytes`], the value Capability Core
/// recomputes from the create body.
pub(super) fn template_digest(template: &Value) -> Result<String, serde_json::Error> {
    let bytes = go_compatible_json_bytes(template)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

/// Refuses a template whose digest would change between create and fire.
///
/// Top-level keys are free: Go re-sorts them every time it hashes, and Postgres
/// keeps them as the values they are. What survives the JSONB round trip is
/// decided below the top level:
///
/// * Nested objects: Postgres JSONB stores keys shortest first (then bytewise)
///   and Go copies a nested object through verbatim, so it hashes nested keys
///   in that order while the create-time digest saw them bytewise. The two
///   orders agree exactly when the bytewise-sorted keys never get shorter, which
///   is true of the `{"goal": ..., "policy": ...}` the Settings form sends and
///   false of `{"ab": ..., "b": ...}`. Flat templates, which is what the chat
///   and new run profile keys use, never have a nested object to get wrong.
/// * Numbers: Go decodes the create body into `float64`, so only integers that
///   float64 holds exactly print the same in both languages.
/// * The reserved fire-intent key, see [`RESERVED_FIRE_INTENT_KEY`].
///
/// The error text names the offending path and is shown to the caller.
pub(super) fn check_digest_stable(template: &Value) -> Result<(), String> {
    let Value::Object(fields) = template else {
        return Err("The task template must be a JSON object.".to_owned());
    };
    if fields.contains_key(RESERVED_FIRE_INTENT_KEY) {
        return Err(format!(
            "The task template cannot use the reserved key `{RESERVED_FIRE_INTENT_KEY}`."
        ));
    }
    for (key, value) in fields {
        check_value(value, key)?;
    }
    Ok(())
}

fn check_value(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::Number(number) => match number.as_i64() {
            Some(integer) if integer.unsigned_abs() <= MAX_SAFE_INTEGER => Ok(()),
            _ => Err(format!(
                "Task template value `{path}` must be an integer between -2^53 and 2^53."
            )),
        },
        Value::Object(fields) => {
            let mut keys: Vec<&str> = fields.keys().map(String::as_str).collect();
            keys.sort_unstable();
            if keys.windows(2).any(|pair| pair[0].len() > pair[1].len()) {
                return Err(format!(
                    "Task template object `{path}` has keys that storage would re-order \
                     (it keeps shorter keys first), which would invalidate the schedule's \
                     digest. Keep the template flat."
                ));
            }
            fields
                .iter()
                .try_for_each(|(key, child)| check_value(child, &format!("{path}.{key}")))
        }
        Value::Array(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| check_value(item, &format!("{path}[{index}]"))),
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}

#[cfg(test)]
pub(super) mod vector {
    //! One template and the digest Go gives it, pinned identically in
    //! capability-core (`workplane_apis_template_digest_test.go`,
    //! `workflow_dispatcher_template_digest_test.go`) and orchestrator-core
    //! (`workflowreg_template_digest_test.go`). Change it in all four places or
    //! none.

    /// The template exactly as a browser could send it: unsorted keys, and a
    /// description carrying every character class where Go's `json.Marshal`
    /// and `serde_json::to_vec` differ or could differ.
    pub(in crate::domains) const TEMPLATE_JSON: &str = r#"{"title":"Ukentlig <rapport> & oppsummering","kind":"cron","description":"Sammenlign \"A\" & B: <b>fet</b> > C. Æ Ø Å æ ø å 日本語 \ud83d\ude00 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\u0008\u000c\u007f slutt","priority":3,"enabled":true,"assignee":null,"x<y&z":"nøkkel"}"#;

    /// Go's `json.Marshal` of [`TEMPLATE_JSON`]. The DEL character (U+007F) is
    /// written as an escape so the literal stays readable.
    pub(in crate::domains) fn go_canonical() -> String {
        [
            r#"{"assignee":null,"description":"Sammenlign \"A\" \u0026 B: \u003cb\u003efet\u003c/b\u003e \u003e C. Æ Ø Å æ ø å 日本語 😀 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\b\f"#,
            "\u{7f}",
            r#" slutt","enabled":true,"kind":"cron","priority":3,"title":"Ukentlig \u003crapport\u003e \u0026 oppsummering","x\u003cy\u0026z":"nøkkel"}"#,
        ]
        .concat()
    }

    /// `sha256` of [`go_canonical`].
    pub(in crate::domains) const GO_DIGEST: &str =
        "sha256:9a15f77fa517dc15cc4a0933b824a31449dbdad178b286dd74d58c573615cd61";
}

#[cfg(test)]
mod tests {
    use super::{
        check_digest_stable, go_compatible_json_bytes, template_digest, vector, MAX_SAFE_INTEGER,
    };
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    fn vector_template() -> Value {
        serde_json::from_str(vector::TEMPLATE_JSON).expect("vector template")
    }

    #[test]
    fn vector_serialises_to_the_bytes_go_marshals() {
        let bytes = go_compatible_json_bytes(&vector_template()).expect("serialises");

        assert_eq!(
            String::from_utf8(bytes).expect("UTF-8"),
            vector::go_canonical()
        );
    }

    #[test]
    fn vector_digest_is_the_digest_go_computes() {
        assert_eq!(
            template_digest(&vector_template()).expect("digest"),
            vector::GO_DIGEST
        );
        // The pinned digest really is the hash of the pinned bytes, so a drift
        // in either constant cannot hide behind the other.
        assert_eq!(
            format!(
                "sha256:{}",
                hex::encode(Sha256::digest(vector::go_canonical().as_bytes()))
            ),
            vector::GO_DIGEST
        );
    }

    #[test]
    fn plain_serde_json_would_not_match_go_for_the_vector() {
        // The defect this module exists to prevent: the digest of serde_json's
        // own bytes is not the digest Go computes for the same template.
        let plain = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(
                serde_json::to_vec(&vector_template()).expect("serialises")
            ))
        );

        assert_ne!(plain, vector::GO_DIGEST);
    }

    #[test]
    fn escapes_html_characters_and_line_separators_in_values_and_keys() {
        let value = json!({"a<b": "<>&\u{2028}\u{2029}"});

        assert_eq!(
            String::from_utf8(go_compatible_json_bytes(&value).expect("serialises")).unwrap(),
            r#"{"a\u003cb":"\u003c\u003e\u0026\u2028\u2029"}"#
        );
    }

    #[test]
    fn leaves_every_other_character_exactly_as_serde_json_writes_it() {
        // Neighbours of the escaped set, the rest of the controls, DEL and
        // astral characters; plus text that merely looks like an escape.
        let value = json!({
            "s": "'\"/\\é\u{7f}\u{2027}\u{202a}\u{1F600}\u{1}\u{8}\u{c}\n\r\t",
            "lookalike": r"\u003c \u2028",
            "n": [1, -2, true, null],
        });

        assert_eq!(
            go_compatible_json_bytes(&value).expect("serialises"),
            serde_json::to_vec(&value).expect("serialises")
        );
    }

    #[test]
    fn serde_json_object_keys_iterate_in_sorted_order() {
        // Go sorts map keys bytewise and the digest depends on the BFF doing
        // the same. serde_json's `Map` is a BTreeMap only while its
        // `preserve_order` feature is off; if a dependency ever turns it on
        // this fails instead of every digest quietly depending on the order a
        // browser happened to send.
        let mut object = serde_json::Map::new();
        for key in ["b", "ab", "é", "a", "Z", "aa"] {
            object.insert(key.to_owned(), Value::Null);
        }

        let keys: Vec<&str> = object.keys().map(String::as_str).collect();

        assert_eq!(keys, ["Z", "a", "aa", "ab", "b", "é"]);
    }

    #[test]
    fn flat_templates_are_stable() {
        assert_eq!(check_digest_stable(&vector_template()), Ok(()));
        assert_eq!(
            check_digest_stable(&json!({
                "kind": "cron", "title": "t", "description": "d", "priority": 3,
                "run_route": "managed", "run_notify": true, "run_model": null,
            })),
            Ok(())
        );
    }

    #[test]
    fn nested_objects_are_stable_only_when_storage_keeps_their_key_order() {
        // The shape the Settings form sends.
        assert_eq!(
            check_digest_stable(&json!({"workflow_input": {"goal": "g", "policy": "p"}})),
            Ok(())
        );
        // Equal-length keys are ordered bytewise by both sides.
        assert_eq!(
            check_digest_stable(&json!({"workflow_input": {"bb": 1, "aa": 2}})),
            Ok(())
        );
        // A shorter key that sorts after a longer one is what storage moves.
        let error = check_digest_stable(&json!({"workflow_input": {"ab": 1, "b": 2}}))
            .expect_err("`b` is stored before `ab`");
        assert!(error.contains("`workflow_input`"), "{error}");
        // The order is judged per nested object, at any depth and inside arrays.
        let nested =
            check_digest_stable(&json!({"a": {"ok": {"ab": 1, "b": 2}}})).expect_err("deep object");
        assert!(nested.contains("`a.ok`"), "{nested}");
        let in_array = check_digest_stable(&json!({"a": [{"x": 1}, {"ab": 1, "b": 2}]}))
            .expect_err("object in array");
        assert!(in_array.contains("`a[1]`"), "{in_array}");
        // Keys are compared by UTF-8 byte length, as storage does: `é` is two
        // bytes, so it follows `ab` (two bytes) but precedes `zzz`.
        assert_eq!(
            check_digest_stable(&json!({"o": {"ab": 1, "é": 2}})),
            Ok(())
        );
        assert!(check_digest_stable(&json!({"o": {"zzz": 1, "é": 2}})).is_err());
    }

    #[test]
    fn only_integers_that_both_languages_print_alike_are_accepted() {
        assert_eq!(check_digest_stable(&json!({"p": 0})), Ok(()));
        assert_eq!(check_digest_stable(&json!({"p": -3})), Ok(()));
        assert_eq!(check_digest_stable(&json!({"p": MAX_SAFE_INTEGER})), Ok(()));
        assert!(check_digest_stable(&json!({"p": MAX_SAFE_INTEGER + 1})).is_err());
        assert!(check_digest_stable(&json!({"p": -((MAX_SAFE_INTEGER + 1) as i64)})).is_err());
        assert!(check_digest_stable(&json!({"p": 1.5})).is_err());
        assert!(check_digest_stable(&json!({"p": 3.0})).is_err());
        assert!(check_digest_stable(&json!({"p": u64::MAX})).is_err());
        assert!(check_digest_stable(&json!({"o": {"p": 1e21}})).is_err());
    }

    #[test]
    fn the_fire_intent_key_is_reserved() {
        assert!(check_digest_stable(&json!({"schedule_fire_intent": {}})).is_err());
        assert!(check_digest_stable(&json!({"o": {"schedule_fire_intent": 1}})).is_ok());
    }

    #[test]
    fn a_template_must_be_an_object() {
        assert!(check_digest_stable(&json!(["a"])).is_err());
        assert!(check_digest_stable(&json!("a")).is_err());
        assert!(check_digest_stable(&Value::Null).is_err());
    }
}
