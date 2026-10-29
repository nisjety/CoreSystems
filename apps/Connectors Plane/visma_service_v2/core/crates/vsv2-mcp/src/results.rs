//! Result handles: when a tool's answer is too big for the model's context it is parked here and
//! the model receives a small envelope with a `result_id` instead. `visma_read_result` then pages
//! through it by JSON pointer. In-memory only, bounded, expiring: a convenience, not a record.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Entries kept at once (oldest evicted first).
pub const MAX_ENTRIES: usize = 32;
/// A result bigger than this cannot be held for paging at all.
pub const MAX_ENTRY_BYTES: usize = 5 * 1024 * 1024;
pub const DEFAULT_TTL: Duration = Duration::from_secs(600);
/// Budget for one page returned by `visma_read_result`.
pub const PAGE_BUDGET_BYTES: usize = 120_000;

struct Entry {
    id: String,
    value: Arc<Value>,
    created: Instant,
}

pub struct ResultStore {
    entries: Mutex<VecDeque<Entry>>,
    ttl: Duration,
}

impl Default for ResultStore {
    fn default() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }
}

impl ResultStore {
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            ttl,
        }
    }

    pub fn ttl_secs(&self) -> u64 {
        self.ttl.as_secs()
    }

    fn purge(&self, q: &mut VecDeque<Entry>) {
        let now = Instant::now();
        q.retain(|e| now.duration_since(e.created) <= self.ttl);
    }

    /// Park a result and return its handle.
    pub fn put(&self, value: Value) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut q = self.entries.lock().expect("result store lock");
        self.purge(&mut q);
        while q.len() >= MAX_ENTRIES {
            q.pop_front();
        }
        q.push_back(Entry {
            id: id.clone(),
            value: Arc::new(value),
            created: Instant::now(),
        });
        id
    }

    pub fn get(&self, id: &str) -> Option<Arc<Value>> {
        let mut q = self.entries.lock().expect("result store lock");
        self.purge(&mut q);
        q.iter().find(|e| e.id == id).map(|e| e.value.clone())
    }
}

/// A compact description of a value's structure, so the model can see what is inside without
/// receiving it.
pub fn shape(v: &Value) -> Value {
    fn one(v: &Value) -> Value {
        match v {
            Value::Array(a) => json!({"type": "array", "len": a.len()}),
            Value::Object(m) => json!({"type": "object", "keys": m.len()}),
            Value::String(s) => json!({"type": "string", "chars": s.chars().count()}),
            Value::Null => json!({"type": "null"}),
            Value::Bool(_) => json!({"type": "bool"}),
            Value::Number(_) => json!({"type": "number"}),
        }
    }
    match v {
        Value::Object(m) => {
            let fields: serde_json::Map<String, Value> = m
                .iter()
                .take(64)
                .map(|(k, v)| (k.clone(), one(v)))
                .collect();
            json!({"type": "object", "keys": m.len(), "fields": fields, "fields_truncated": m.len() > 64})
        }
        Value::Array(a) => {
            let item_keys: Vec<&String> = a
                .first()
                .and_then(Value::as_object)
                .map(|o| o.keys().take(64).collect())
                .unwrap_or_default();
            json!({"type": "array", "len": a.len(), "item_keys": item_keys})
        }
        other => one(other),
    }
}

fn size_of(v: &Value) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(usize::MAX)
}

/// Resolve `pointer` (RFC 6901; empty = the whole value) and return one bounded page of it.
/// Arrays are sliced by `offset`/`limit` (and cut earlier if the page would exceed the budget),
/// strings by characters, objects return their shape (and the value itself when it is small).
pub fn read(value: &Value, pointer: &str, offset: usize, limit: usize) -> Result<Value, String> {
    let target = if pointer.is_empty() {
        Some(value)
    } else {
        value.pointer(pointer)
    };
    let target =
        target.ok_or_else(|| format!("pointer {pointer:?} does not exist in this result"))?;
    match target {
        Value::Array(items) => {
            let total = items.len();
            let mut page: Vec<&Value> = Vec::new();
            let mut used = 0usize;
            for item in items.iter().skip(offset).take(limit) {
                let s = size_of(item);
                if !page.is_empty() && used + s > PAGE_BUDGET_BYTES {
                    break;
                }
                used += s;
                page.push(item);
            }
            let next = offset + page.len();
            Ok(json!({
                "pointer": pointer, "kind": "array", "total": total, "offset": offset,
                "returned": page.len(), "items": page,
                "next_offset": if next < total { json!(next) } else { Value::Null },
            }))
        }
        Value::String(s) => {
            let total = s.chars().count();
            let text: String = s.chars().skip(offset).take(limit.min(20_000)).collect();
            let next = offset + text.chars().count();
            Ok(json!({
                "pointer": pointer, "kind": "string", "total_chars": total, "offset": offset, "text": text,
                "next_offset": if next < total { json!(next) } else { Value::Null },
            }))
        }
        Value::Object(_) => {
            let small = size_of(target) <= PAGE_BUDGET_BYTES;
            Ok(json!({
                "pointer": pointer, "kind": "object", "shape": shape(target),
                "value": if small { target.clone() } else { Value::Null },
                "hint": if small { Value::Null } else { json!("too large to inline: read a child with a deeper JSON pointer, e.g. /rows") },
            }))
        }
        scalar => Ok(json!({"pointer": pointer, "kind": "scalar", "value": scalar})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_expire_and_evict_oldest() {
        let s = ResultStore::with_ttl(Duration::from_millis(40));
        let id = s.put(json!({"a": 1}));
        assert_eq!(s.get(&id).unwrap()["a"], 1);
        std::thread::sleep(Duration::from_millis(60));
        assert!(s.get(&id).is_none(), "expired");

        let s = ResultStore::default();
        let first = s.put(json!(0));
        for i in 1..=MAX_ENTRIES {
            s.put(json!(i));
        }
        assert!(
            s.get(&first).is_none(),
            "the oldest was evicted at capacity"
        );
        assert!(s.get("nope").is_none());
    }

    #[test]
    fn arrays_are_paged_with_a_next_offset() {
        let v = json!({"rows": (0..10).collect::<Vec<_>>()});
        let p = read(&v, "/rows", 0, 4).unwrap();
        assert_eq!(
            (
                p["total"].as_u64(),
                p["returned"].as_u64(),
                p["next_offset"].as_u64()
            ),
            (Some(10), Some(4), Some(4))
        );
        let last = read(&v, "/rows", 8, 4).unwrap();
        assert_eq!(last["items"], json!([8, 9]));
        assert!(last["next_offset"].is_null());
        assert_eq!(read(&v, "/rows", 99, 4).unwrap()["returned"], 0);
    }

    #[test]
    fn a_page_is_cut_to_the_budget_but_always_makes_progress() {
        let big = "x".repeat(70_000);
        let v = json!([big.clone(), big.clone(), big]);
        let p = read(&v, "", 0, 500).unwrap();
        assert_eq!(p["returned"], 1, "two 70KB items exceed the page budget");
        assert_eq!(p["next_offset"], 1);
    }

    #[test]
    fn strings_objects_scalars_and_bad_pointers() {
        let v = json!({"s": "héllo wörld", "o": {"k": 1}, "n": 5});
        let s = read(&v, "/s", 6, 3).unwrap();
        assert_eq!(s["text"], "wör");
        assert_eq!(s["next_offset"], 9);
        let o = read(&v, "/o", 0, 10).unwrap();
        assert_eq!(o["value"], json!({"k": 1}));
        assert_eq!(read(&v, "/n", 0, 1).unwrap()["value"], 5);
        assert!(read(&v, "/missing", 0, 1)
            .unwrap_err()
            .contains("does not exist"));
    }

    #[test]
    fn shape_describes_structure_without_the_data() {
        let sh = shape(&json!({"rows": [1, 2, 3], "meta": {"a": 1}, "note": "abc"}));
        assert_eq!(sh["fields"]["rows"], json!({"type": "array", "len": 3}));
        assert_eq!(sh["fields"]["note"], json!({"type": "string", "chars": 3}));
        let arr = shape(&json!([{"id": 1, "x": 2}]));
        assert_eq!(arr["len"], 1);
        assert_eq!(arr["item_keys"].as_array().unwrap().len(), 2);
    }
}
