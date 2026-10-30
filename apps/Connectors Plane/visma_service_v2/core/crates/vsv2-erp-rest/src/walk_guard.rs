//! Completeness check for a page-by-page walk.
//!
//! A page-number walk over live data can silently lose a row. `/v1/payment` is ordered by
//! `refNbr` only, so `Payment 408853` and `VoidPayment 408853` (same number) straddle a page
//! boundary and the two page requests may order the tie differently: one row comes back twice,
//! the other never. The API's `totalCount` still equals the number of rows received, so nothing
//! looks wrong. Two full walks each returned 21,392 rows but only 21,391 distinct payments.
//!
//! What gives it away is `totalCount` compared with the *distinct* rows received. This module
//! decides whether a walk fell short, and merges a second walk (made with other page
//! boundaries) into the first by adding only the keys the first walk lacks. It never replaces or
//! removes a row from the first walk: two different rows with one key must still reach the
//! executor, whose validation rejects them as the key bug they are.

use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

use serde_json::Value;

/// How many extra walks are made before giving up.
pub(crate) const MAX_REWALKS: usize = 2;

/// Distinct keys and exact duplicates in the rows of one or more walks.
#[derive(Debug, Default)]
pub(crate) struct Tally {
    /// key -> hash of the whole record first seen under that key.
    seen: HashMap<String, u64>,
    pub(crate) rows: usize,
    /// Rows identical to one already seen: evidence of a page boundary re-serving a row.
    pub(crate) exact_duplicates: usize,
}

impl Tally {
    pub(crate) fn distinct_keys(&self) -> usize {
        self.seen.len()
    }

    pub(crate) fn contains(&self, key: &str) -> bool {
        self.seen.contains_key(key)
    }

    /// Count one record. `false` when it has no usable key, which disables the check.
    pub(crate) fn add(&mut self, record: &Value, key_fields: &[String]) -> bool {
        let Some(key) = record_key(record, key_fields) else {
            return false;
        };
        let digest = digest(record);
        self.rows += 1;
        match self.seen.get(&key) {
            Some(existing) if *existing == digest => self.exact_duplicates += 1,
            Some(_) => {} // same key, different row: a key problem for validation, not for us
            None => {
                self.seen.insert(key, digest);
            }
        }
        true
    }
}

/// The record's key as one string, or `None` when any part is missing or empty.
pub(crate) fn record_key(record: &Value, fields: &[String]) -> Option<String> {
    let mut key = String::new();
    for (i, field) in fields.iter().enumerate() {
        let mut current = record;
        for part in field.split('.') {
            current = current.get(part)?;
        }
        let text = match current {
            Value::String(s) => s.trim().to_string(),
            Value::Null => return None,
            other => other.to_string(),
        };
        if text.is_empty() {
            return None;
        }
        if i > 0 {
            key.push('\u{1f}');
        }
        key.push_str(&text);
    }
    (!fields.is_empty()).then_some(key)
}

fn digest(record: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    record.to_string().hash(&mut hasher);
    hasher.finish()
}

/// Tally a whole list of rows; `None` when a row has no key (the check cannot be trusted). A walk
/// tallies its rows one page at a time, so only the tests need a whole list tallied at once.
#[cfg(test)]
pub(crate) fn tally(records: &[Value], key_fields: &[String]) -> Option<Tally> {
    let mut tally = Tally::default();
    for record in records {
        if !tally.add(record, key_fields) {
            return None;
        }
    }
    Some(tally)
}

/// Whether the walk lost rows, judged against the API's own total.
///
/// Fewer distinct keys than `totalCount` is only a shortfall when the walk shows it: fewer rows
/// than the total, or a row served twice. Rows equal to the total with no exact duplicate and
/// still fewer distinct keys means different rows share a key; that is a configuration
/// problem, and re-walking cannot fix it.
pub(crate) fn fell_short(total: Option<usize>, tally: &Tally) -> bool {
    match total {
        Some(total) => {
            total > tally.distinct_keys() && (tally.rows < total || tally.exact_duplicates > 0)
        }
        None => false,
    }
}

/// Whether a repair walk's `record` is a row the first walk did not deliver, and if so counts it.
///
/// A row without a key is not admitted, and a key the tally already holds never is, so the rows
/// the first walk delivered are not handed over again.
pub(crate) fn admit(tally: &mut Tally, record: &Value, key_fields: &[String]) -> bool {
    let Some(key) = record_key(record, key_fields) else {
        return false;
    };
    if tally.contains(&key) {
        return false;
    }
    tally.add(record, key_fields);
    true
}

/// Page sizes for the extra walks: smaller than the first, so they stay under the API maximum,
/// and different, so the page boundaries fall on other rows.
pub(crate) fn rewalk_page_sizes(page_size: u32) -> Vec<u32> {
    let mut sizes: Vec<u32> = Vec::new();
    for size in [page_size * 7 / 8, page_size * 3 / 4] {
        if size >= 1 && size != page_size && !sizes.contains(&size) {
            sizes.push(size);
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fields(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn payment(kind: &str, number: &str) -> Value {
        json!({"type": kind, "refNbr": number, "amount": 1.0})
    }

    #[test]
    fn key_is_built_from_dotted_paths_and_trimmed() {
        let record = json!({"account": {"number": " 1080 "}, "period": "202301"});
        let key = record_key(&record, &fields(&["account.number", "period"])).unwrap();
        assert_eq!(key, "1080\u{1f}202301");
    }

    #[test]
    fn a_missing_or_empty_key_part_gives_no_key() {
        let fields = fields(&["type", "refNbr"]);
        assert!(record_key(&json!({"type": "Payment"}), &fields).is_none());
        assert!(record_key(&json!({"type": "Payment", "refNbr": null}), &fields).is_none());
        assert!(record_key(&json!({"type": "Payment", "refNbr": "  "}), &fields).is_none());
        assert!(record_key(&json!({}), &[]).is_none());
    }

    #[test]
    fn a_complete_walk_is_not_a_shortfall() {
        let rows = vec![payment("Payment", "1"), payment("Payment", "2")];
        let t = tally(&rows, &fields(&["type", "refNbr"])).unwrap();
        assert!(!fell_short(Some(2), &t));
    }

    #[test]
    fn a_walk_with_fewer_rows_than_the_total_is_a_shortfall() {
        let rows = vec![payment("Payment", "1")];
        let t = tally(&rows, &fields(&["type", "refNbr"])).unwrap();
        assert!(fell_short(Some(2), &t));
    }

    #[test]
    fn a_row_served_twice_while_another_is_skipped_is_a_shortfall() {
        // 3 rows received == totalCount 3, but only 2 distinct: the payments incident.
        let rows = vec![
            payment("Payment", "1"),
            payment("VoidPayment", "1"),
            payment("VoidPayment", "1"),
        ];
        let t = tally(&rows, &fields(&["type", "refNbr"])).unwrap();
        assert_eq!(t.rows, 3);
        assert_eq!(t.distinct_keys(), 2);
        assert!(fell_short(Some(3), &t));
    }

    #[test]
    fn different_rows_sharing_a_key_are_left_to_validation() {
        // The sales-order incident: key `orderId` alone, two order types with one number.
        let a = json!({"orderId": "1", "type": "SO"});
        let b = json!({"orderId": "1", "type": "IO"});
        let t = tally(&[a, b], &fields(&["orderId"])).unwrap();
        assert_eq!(t.rows, 2);
        assert_eq!(t.distinct_keys(), 1);
        assert_eq!(t.exact_duplicates, 0);
        assert!(
            !fell_short(Some(2), &t),
            "re-walking cannot fix a wrong key"
        );
    }

    #[test]
    fn no_total_means_no_verdict() {
        let t = tally(&[payment("Payment", "1")], &fields(&["type", "refNbr"])).unwrap();
        assert!(!fell_short(None, &t));
    }

    #[test]
    fn a_keyless_record_disables_the_check() {
        let rows = vec![payment("Payment", "1"), json!({"type": "Payment"})];
        assert!(tally(&rows, &fields(&["type", "refNbr"])).is_none());
    }

    #[test]
    fn admit_takes_only_rows_whose_key_the_first_walk_lacks() {
        let key = fields(&["type", "refNbr"]);
        let first = vec![
            payment("Payment", "1"),
            payment("VoidPayment", "1"),
            payment("VoidPayment", "1"),
        ];
        let mut t = tally(&first, &key).unwrap();
        let second = vec![
            payment("Payment", "1"),
            payment("VoidPayment", "1"),
            payment("Payment", "2"), // the row the first walk never saw
            payment("Payment", "2"), // twice in the same walk: admitted once
        ];

        let admitted: Vec<Value> = second
            .into_iter()
            .filter(|record| admit(&mut t, record, &key))
            .collect();

        assert_eq!(admitted, vec![payment("Payment", "2")]);
        assert_eq!(t.distinct_keys(), 3);
        assert!(!fell_short(Some(3), &t));
    }

    #[test]
    fn a_keyless_row_is_never_admitted() {
        let key = fields(&["type", "refNbr"]);
        let mut t = tally(&[payment("Payment", "1")], &key).unwrap();

        assert!(!admit(&mut t, &json!({"type": "Payment"}), &key));
        assert_eq!(t.distinct_keys(), 1);
    }

    #[test]
    fn rewalk_sizes_differ_from_the_original_and_stay_below_it() {
        assert_eq!(rewalk_page_sizes(100), vec![87, 75]);
        assert_eq!(rewalk_page_sizes(1000), vec![875, 750]);
        assert_eq!(rewalk_page_sizes(1), Vec::<u32>::new());
        assert!(rewalk_page_sizes(15).iter().all(|s| *s < 15 && *s >= 1));
    }
}
