//! The per-user attention stream of Session Core (build plan WS16 S5).
//!
//! One stream per signed-in person carries the closed attention state of that person's threads: a
//! snapshot first, then one frame per thread that changed, a resync when the rows could not be read,
//! and a heartbeat while nothing changes. Frames carry identifiers and the closed state only. There is
//! no replay buffer: a client that reconnects starts again from a snapshot.
//!
//! This module holds the parts that touch neither the database nor the wire: the diff between two
//! listings, the sequence stamped on every frame, the per-person stream cap and the epoch of this
//! process. `attention_grpc.rs` runs the loop that reads the rows and sends the frames.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The gap between two heartbeat frames on a stream that has nothing else to say.
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(20);

/// The most streams one person may hold at once in this process. A stream past the cap is refused
/// rather than queued, so one browser profile with many tabs cannot exhaust the replica.
pub(crate) const MAX_STREAMS_PER_USER: usize = 4;

/// The identity of this process's streams. A restarted replica answers with a new epoch, so a client
/// can tell that the sequence numbers it kept belong to a process that no longer exists.
pub(crate) static EPOCH: LazyLock<String> = LazyLock::new(|| {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{:x}-{:x}", std::process::id(), nanos)
});

/// The streams this process holds open, shared by every stream handler.
pub(crate) static STREAMS: LazyLock<Arc<StreamLimiter>> =
    LazyLock::new(|| StreamLimiter::new(MAX_STREAMS_PER_USER));

/// A listed thread, keyed by its thread id. The diff compares items whole, so an item that differs in
/// any field is sent again.
pub(crate) trait Keyed {
    fn key(&self) -> &str;
}

/// One listing of a person's threads and the counts behind the badge.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listing<I, C> {
    pub(crate) items: Vec<I>,
    pub(crate) counts: C,
}

/// What changed for one thread.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Change<I> {
    /// The thread is listed with this item: new, or changed.
    Upsert(I),
    /// The thread left the listing.
    Remove(String),
}

/// The body of one frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Body<I, C> {
    /// The whole listing. The first frame of every stream, and the only frame that replaces the counts
    /// on its own.
    Snapshot(Listing<I, C>),
    /// One thread's change, with the counts after it.
    Changed { change: Change<I>, counts: C },
    /// The rows could not be read: discard what you hold, a snapshot follows.
    Resync,
    /// Nothing changed for a while; the stream is alive.
    Heartbeat,
}

/// One frame with its place in the stream.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frame<I, C> {
    pub(crate) epoch: String,
    pub(crate) seq: u64,
    pub(crate) body: Body<I, C>,
}

/// Numbers the frames of one stream. The first frame is 1 and each frame after it is one more, so a
/// client that sees a gap knows frames were lost.
pub(crate) struct Sequencer {
    epoch: String,
    last: u64,
}

impl Sequencer {
    pub(crate) fn new(epoch: &str) -> Self {
        Self {
            epoch: epoch.to_owned(),
            last: 0,
        }
    }

    pub(crate) fn stamp<I, C>(&mut self, body: Body<I, C>) -> Frame<I, C> {
        self.last += 1;
        Frame {
            epoch: self.epoch.clone(),
            seq: self.last,
            body,
        }
    }
}

/// The bodies that bring a client from `previous`, what the stream last described (`None` before its
/// first snapshot), to `next`. Unchanged threads are not sent. A change to the counts with no change to
/// any thread is sent as a snapshot, because the counts only travel whole.
pub(crate) fn diff<I, C>(previous: Option<&Listing<I, C>>, next: Listing<I, C>) -> Vec<Body<I, C>>
where
    I: Keyed + Clone + PartialEq,
    C: Clone + PartialEq,
{
    let Some(previous) = previous else {
        return vec![Body::Snapshot(next)];
    };
    let changes = {
        let before: BTreeMap<&str, &I> = previous
            .items
            .iter()
            .map(|item| (item.key(), item))
            .collect();
        let after: BTreeMap<&str, &I> = next.items.iter().map(|item| (item.key(), item)).collect();
        let mut changes: Vec<Change<I>> = next
            .items
            .iter()
            .filter(|item| before.get(item.key()) != Some(item))
            .map(|item| Change::Upsert(item.clone()))
            .collect();
        changes.extend(
            previous
                .items
                .iter()
                .filter(|item| !after.contains_key(item.key()))
                .map(|item| Change::Remove(item.key().to_owned())),
        );
        changes
    };
    if changes.is_empty() {
        return if previous.counts == next.counts {
            Vec::new()
        } else {
            vec![Body::Snapshot(next)]
        };
    }
    changes
        .into_iter()
        .map(|change| Body::Changed {
            change,
            counts: next.counts.clone(),
        })
        .collect()
}

/// The streams open per person (organization and user) in this process, and the cap on them.
pub(crate) struct StreamLimiter {
    limit: usize,
    open: Mutex<HashMap<(String, String), usize>>,
}

impl StreamLimiter {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            open: Mutex::new(HashMap::new()),
        })
    }

    /// A permit for one more stream of this person, or `None` when the person already holds the cap.
    /// The organization is part of the key, so one user id in two organizations has two caps.
    pub(crate) fn acquire(self: &Arc<Self>, org_id: &str, user_id: &str) -> Option<StreamPermit> {
        let key = (org_id.to_owned(), user_id.to_owned());
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let count = open.entry(key.clone()).or_insert(0);
        if *count >= self.limit {
            return None;
        }
        *count += 1;
        Some(StreamPermit {
            limiter: Arc::clone(self),
            key,
        })
    }

    fn release(&self, key: &(String, String)) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = open.get_mut(key) {
            *count -= 1;
            if *count == 0 {
                open.remove(key);
            }
        }
    }

    #[cfg(test)]
    fn open_streams(&self, org_id: &str, user_id: &str) -> usize {
        let open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.get(&(org_id.to_owned(), user_id.to_owned()))
            .copied()
            .unwrap_or(0)
    }
}

/// One open stream. Dropping it frees its slot.
pub(crate) struct StreamPermit {
    limiter: Arc<StreamLimiter>,
    key: (String, String),
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.limiter.release(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Item {
        id: &'static str,
        state: u8,
    }

    impl Keyed for Item {
        fn key(&self) -> &str {
            self.id
        }
    }

    fn item(id: &'static str, state: u8) -> Item {
        Item { id, state }
    }

    fn listing(items: Vec<Item>, counts: (i32, i32)) -> Listing<Item, (i32, i32)> {
        Listing { items, counts }
    }

    #[test]
    fn the_first_listing_is_a_snapshot() {
        let first = listing(vec![item("a", 1)], (1, 0));
        assert_eq!(
            diff(None, first.clone()),
            vec![Body::Snapshot(first)],
            "a stream opens with the whole listing"
        );
    }

    #[test]
    fn an_unchanged_listing_sends_nothing() {
        let steady = listing(vec![item("a", 1), item("b", 2)], (1, 1));
        assert!(diff(Some(&steady), steady.clone()).is_empty());
    }

    #[test]
    fn only_new_and_changed_threads_are_sent_with_the_counts_after_them() {
        let before = listing(vec![item("a", 1), item("b", 1)], (1, 0));
        let after = listing(vec![item("a", 2), item("b", 1), item("c", 1)], (2, 0));
        assert_eq!(
            diff(Some(&before), after),
            vec![
                Body::Changed {
                    change: Change::Upsert(item("a", 2)),
                    counts: (2, 0),
                },
                Body::Changed {
                    change: Change::Upsert(item("c", 1)),
                    counts: (2, 0),
                },
            ]
        );
    }

    #[test]
    fn a_thread_that_left_the_listing_is_removed() {
        let before = listing(vec![item("a", 1), item("b", 1)], (2, 0));
        let after = listing(vec![item("a", 1)], (2, 0));
        assert_eq!(
            diff(Some(&before), after),
            vec![Body::Changed {
                change: Change::Remove("b".to_owned()),
                counts: (2, 0),
            }]
        );
    }

    #[test]
    fn a_count_change_with_no_thread_change_resends_the_snapshot() {
        let before = listing(vec![item("a", 1)], (1, 0));
        let after = listing(vec![item("a", 1)], (0, 0));
        assert_eq!(
            diff(Some(&before), after.clone()),
            vec![Body::Snapshot(after)]
        );
    }

    #[test]
    fn the_sequence_starts_at_one_and_rises_by_one_within_an_epoch() {
        let mut sequencer = Sequencer::new("epoch-test");
        let frames: Vec<Frame<Item, (i32, i32)>> =
            (0..3).map(|_| sequencer.stamp(Body::Heartbeat)).collect();
        assert_eq!(
            frames.iter().map(|frame| frame.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(frames.iter().all(|frame| frame.epoch == "epoch-test"));
    }

    #[test]
    fn the_limiter_refuses_the_next_stream_past_the_cap_and_frees_a_slot_on_drop() {
        let limiter = StreamLimiter::new(2);
        let first = limiter.acquire("org-1", "user-1").expect("first stream");
        let _second = limiter.acquire("org-1", "user-1").expect("second stream");
        assert!(
            limiter.acquire("org-1", "user-1").is_none(),
            "a third stream is refused"
        );
        drop(first);
        assert!(limiter.acquire("org-1", "user-1").is_some());
    }

    #[test]
    fn the_cap_is_per_organization_and_per_user() {
        let limiter = StreamLimiter::new(1);
        let _held = limiter
            .acquire("org-1", "user-1")
            .expect("the first stream");
        assert!(
            limiter.acquire("org-2", "user-1").is_some(),
            "the same user id in another organization has its own cap"
        );
        assert!(
            limiter.acquire("org-1", "user-2").is_some(),
            "another user in the same organization has their own cap"
        );
        assert_eq!(limiter.open_streams("org-1", "user-1"), 1);
    }

    #[test]
    fn the_permit_count_returns_to_zero_when_the_last_stream_closes() {
        let limiter = StreamLimiter::new(4);
        let held = limiter.acquire("org-1", "user-1").expect("a stream");
        assert_eq!(limiter.open_streams("org-1", "user-1"), 1);
        drop(held);
        assert_eq!(limiter.open_streams("org-1", "user-1"), 0);
    }

    #[test]
    fn the_heartbeat_and_the_cap_are_the_documented_values() {
        assert_eq!(HEARTBEAT, Duration::from_secs(20));
        assert_eq!(MAX_STREAMS_PER_USER, 4);
    }

    #[test]
    fn the_epoch_is_one_value_for_the_life_of_the_process() {
        assert!(!EPOCH.is_empty());
        assert_eq!(EPOCH.as_str(), EPOCH.as_str());
        assert!(EPOCH.contains('-'));
    }
}
