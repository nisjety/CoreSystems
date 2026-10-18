//! Bookkeeping for the local browser: who holds a session, on which browser
//! process ("slot"), how many pages each slot has open, and which sessions have
//! been idle or alive too long.
//!
//! Pure data: no chromiumoxide types, no clock of its own (every method takes
//! `now`), no `.await`. The driver keeps one `SlotBook` behind a
//! `std::sync::Mutex` and never holds that lock across an await, so the book is
//! always the innermost lock.
//!
//! Today the driver runs one slot (K = 1, ADR-10); the book already chooses the
//! least loaded slot so the pool (WS3.G) only has to raise the count.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Which browser process a session lives in.
pub type SlotId = usize;

/// How long a lost session is remembered, so its next call can say why it is
/// gone instead of "no page".
pub const TOMBSTONE_TTL: Duration = Duration::from_secs(600);

/// What a refused admission tells the caller to wait.
pub const BUSY_RETRY_AFTER_S: u64 = 15;

/// Two failed probes in a row make a slot unhealthy.
pub const PROBE_FAILURES_UNHEALTHY: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    /// No browser yet (generation 0) or one is being started.
    Starting,
    Ready,
    /// Takes no new sessions; closed once empty (WS3.G).
    Retiring,
    /// The process is gone; the next admission relaunches it.
    Dead,
}

impl SlotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Retiring => "retiring",
            Self::Dead => "dead",
        }
    }

    /// Preference when two slots carry the same load.
    fn rank(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Starting => 1,
            Self::Dead => 2,
            Self::Retiring => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotState {
    pub id: SlotId,
    pub status: SlotStatus,
    /// How many times this slot has been launched; 0 = never.
    pub generation: u64,
    pub sessions: usize,
    pub pages: usize,
    pub probe_failures: u32,
    /// `None` until the first probe answers.
    pub healthy: Option<bool>,
}

impl SlotState {
    fn new(id: SlotId) -> Self {
        Self {
            id,
            status: SlotStatus::Starting,
            generation: 0,
            sessions: 0,
            pages: 0,
            probe_failures: 0,
            healthy: None,
        }
    }

    pub fn launched(&self) -> bool {
        self.generation > 0
    }
}

/// The limits the book enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotConfig {
    pub slots: usize,
    /// 0 = no cap.
    pub max_sessions: usize,
    /// 0 = no cap.
    pub max_pages_per_slot: usize,
    pub session_idle: Duration,
    pub session_max_age: Duration,
}

impl Default for SlotConfig {
    fn default() -> Self {
        BrowserPoolConfig::default().slot_config()
    }
}

/// `QUARRY_BROWSER_REAPER`: what the idle reaper does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaperMode {
    /// No reaper and no prober. Caps and crash handling stay on.
    Off,
    /// Log what would be reaped, close nothing (the first week).
    Log,
    On,
}

impl ReaperMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "0",
            Self::Log => "log",
            Self::On => "1",
        }
    }
}

/// Every `QUARRY_BROWSER_*` knob for the local browser pool, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserPoolConfig {
    pub max_sessions: usize,
    pub max_pages_per_slot: usize,
    pub session_idle: Duration,
    pub session_max_age: Duration,
    pub reaper_interval: Duration,
    pub probe_interval: Duration,
    pub reaper: ReaperMode,
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            max_sessions: 40,
            max_pages_per_slot: 24,
            session_idle: Duration::from_secs(3_600),
            session_max_age: Duration::from_secs(21_600),
            reaper_interval: Duration::from_secs(30),
            probe_interval: Duration::from_secs(15),
            reaper: ReaperMode::Log,
        }
    }
}

const MAX_CAP: u64 = 4_096;

impl BrowserPoolConfig {
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// `from_env` over any lookup, for tests. Out-of-range numbers are clamped;
    /// text that is not a number falls back to the default with a warning.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let defaults = Self::default();
        let number = |name: &str, default: u64, min: u64, max: u64| -> u64 {
            match lookup(name).map(|raw| raw.trim().to_owned()) {
                None => default,
                Some(raw) if raw.is_empty() => default,
                Some(raw) => match raw.parse::<u64>() {
                    Ok(value) => value.clamp(min, max),
                    Err(_) => {
                        tracing::warn!(knob = name, "browser pool setting is not a number; using the default");
                        default
                    }
                },
            }
        };
        let max_sessions = number(
            "QUARRY_BROWSER_MAX_SESSIONS",
            defaults.max_sessions as u64,
            0,
            MAX_CAP,
        ) as usize;
        let max_pages_per_slot = number(
            "QUARRY_BROWSER_MAX_PAGES_PER_SLOT",
            defaults.max_pages_per_slot as u64,
            0,
            MAX_CAP,
        ) as usize;
        let idle_s = number(
            "QUARRY_BROWSER_SESSION_IDLE_S",
            defaults.session_idle.as_secs(),
            30,
            7 * 86_400,
        );
        let max_age_s = number(
            "QUARRY_BROWSER_SESSION_MAX_AGE_S",
            defaults.session_max_age.as_secs(),
            60,
            30 * 86_400,
        )
        // An age limit below the idle limit would reap busy sessions first.
        .max(idle_s);
        let reaper_interval_s = number(
            "QUARRY_BROWSER_REAPER_INTERVAL_S",
            defaults.reaper_interval.as_secs(),
            1,
            3_600,
        );
        let probe_interval_s = number(
            "QUARRY_BROWSER_PROBE_INTERVAL_S",
            defaults.probe_interval.as_secs(),
            1,
            3_600,
        );
        let reaper = match lookup("QUARRY_BROWSER_REAPER")
            .map(|raw| raw.trim().to_ascii_lowercase())
            .as_deref()
        {
            None | Some("") | Some("log") => ReaperMode::Log,
            Some("0") | Some("off") | Some("false") => ReaperMode::Off,
            Some("1") | Some("on") | Some("true") => ReaperMode::On,
            Some(_) => {
                tracing::warn!("QUARRY_BROWSER_REAPER must be 0, log or 1; using log");
                ReaperMode::Log
            }
        };
        Self {
            max_sessions,
            max_pages_per_slot,
            session_idle: Duration::from_secs(idle_s),
            session_max_age: Duration::from_secs(max_age_s),
            reaper_interval: Duration::from_secs(reaper_interval_s),
            probe_interval: Duration::from_secs(probe_interval_s),
            reaper,
        }
    }

    /// The book's view of this config, for `slots` browser processes.
    pub fn slot_config(&self) -> SlotConfig {
        SlotConfig {
            slots: 1,
            max_sessions: self.max_sessions,
            max_pages_per_slot: self.max_pages_per_slot,
            session_idle: self.session_idle,
            session_max_age: self.session_max_age,
        }
    }
}

/// Why the book let go of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReapReason {
    Idle,
    MaxAge,
}

impl ReapReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::MaxAge => "max_age",
        }
    }
}

/// Why a session that was admitted is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossReason {
    Reaped(ReapReason),
    ProcessLost,
}

impl LossReason {
    /// The `details.reason` the driver answers with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reaped(_) => "session_reaped",
            Self::ProcessLost => "browser_process_lost",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admitted {
        slot: SlotId,
        /// The slot has no live browser: the caller launches one.
        needs_launch: bool,
        /// False when this key was already admitted (a second acquire for the
        /// same session): a rollback must not undo the first one.
        fresh: bool,
    },
    Busy {
        limit: usize,
        active: usize,
        retry_after_s: u64,
    },
}

/// A refused page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRefusal {
    /// The slot holds `limit` pages already.
    Cap { limit: usize, active: usize },
    /// The session is not admitted (released, reaped or lost).
    NotAdmitted(Option<LossReason>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaped {
    pub session_key: String,
    pub slot: SlotId,
    pub reason: ReapReason,
    /// Pages the session still held.
    pub pages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Released {
    pub slot: SlotId,
    pub pages: usize,
}

#[derive(Debug, Clone)]
struct SessionEntry {
    slot: SlotId,
    admitted_at: Instant,
    last_touch: Instant,
    pages: usize,
    idle_limit: Duration,
    age_limit: Duration,
    reap_reported: bool,
}

#[derive(Debug)]
pub struct SlotBook {
    cfg: SlotConfig,
    slots: Vec<SlotState>,
    sessions: HashMap<String, SessionEntry>,
    tombstones: HashMap<String, (Instant, LossReason)>,
}

impl SlotBook {
    pub fn new(cfg: SlotConfig) -> Self {
        let count = cfg.slots.max(1);
        Self {
            cfg,
            slots: (0..count).map(SlotState::new).collect(),
            sessions: HashMap::new(),
            tombstones: HashMap::new(),
        }
    }

    pub fn config(&self) -> SlotConfig {
        self.cfg
    }

    pub fn slot(&self, slot: SlotId) -> Option<&SlotState> {
        self.slots.get(slot)
    }

    pub fn slots(&self) -> &[SlotState] {
        &self.slots
    }

    pub fn sessions_open(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_admitted(&self, key: &str) -> bool {
        self.sessions.contains_key(key)
    }

    pub fn session_slot(&self, key: &str) -> Option<SlotId> {
        self.sessions.get(key).map(|entry| entry.slot)
    }

    /// Admit a session. `ttl` is the lease's own time to live; a session is
    /// never reaped before twice that.
    pub fn admit(&mut self, key: &str, ttl: Duration, now: Instant) -> Admission {
        self.prune_tombstones(now);
        if let Some(entry) = self.sessions.get_mut(key) {
            entry.last_touch = now;
            entry.reap_reported = false;
            let slot = entry.slot;
            return Admission::Admitted {
                slot,
                needs_launch: self.slots[slot].status != SlotStatus::Ready,
                fresh: false,
            };
        }
        let active = self.sessions.len();
        if self.cfg.max_sessions > 0 && active >= self.cfg.max_sessions {
            return Admission::Busy {
                limit: self.cfg.max_sessions,
                active,
                retry_after_s: BUSY_RETRY_AFTER_S,
            };
        }
        let Some(slot) = self
            .slots
            .iter()
            .filter(|slot| slot.status != SlotStatus::Retiring)
            .min_by_key(|slot| (slot.sessions, slot.status.rank(), slot.id))
            .map(|slot| slot.id)
        else {
            return Admission::Busy {
                limit: self.cfg.max_sessions,
                active,
                retry_after_s: BUSY_RETRY_AFTER_S,
            };
        };
        self.tombstones.remove(key);
        let twice_ttl = ttl.saturating_mul(2);
        self.sessions.insert(
            key.to_owned(),
            SessionEntry {
                slot,
                admitted_at: now,
                last_touch: now,
                pages: 0,
                idle_limit: twice_ttl.max(self.cfg.session_idle),
                age_limit: twice_ttl.max(self.cfg.session_max_age),
                reap_reported: false,
            },
        );
        self.slots[slot].sessions += 1;
        Admission::Admitted {
            slot,
            needs_launch: self.slots[slot].status != SlotStatus::Ready,
            fresh: true,
        }
    }

    /// The slot's browser is up: a new generation, healthy until probed.
    pub fn launched(&mut self, slot: SlotId) -> u64 {
        let state = &mut self.slots[slot];
        state.generation += 1;
        state.status = SlotStatus::Ready;
        state.probe_failures = 0;
        state.healthy = None;
        state.generation
    }

    /// Reserve a page for `key` on its slot.
    pub fn page_opened(&mut self, key: &str, now: Instant) -> Result<SlotId, PageRefusal> {
        self.prune_tombstones(now);
        let Some(entry) = self.sessions.get(key) else {
            return Err(PageRefusal::NotAdmitted(
                self.tombstones.get(key).map(|(_, reason)| *reason),
            ));
        };
        let slot = entry.slot;
        let pages = self.slots[slot].pages;
        if self.cfg.max_pages_per_slot > 0 && pages >= self.cfg.max_pages_per_slot {
            return Err(PageRefusal::Cap {
                limit: self.cfg.max_pages_per_slot,
                active: pages,
            });
        }
        let entry = self.sessions.get_mut(key).expect("checked above");
        entry.pages += 1;
        entry.last_touch = now;
        entry.reap_reported = false;
        self.slots[slot].pages += 1;
        Ok(slot)
    }

    /// Give a page back. A session that is already gone took its pages with it.
    pub fn page_closed(&mut self, key: &str) {
        if let Some(entry) = self.sessions.get_mut(key) {
            if entry.pages > 0 {
                entry.pages -= 1;
                let slot = &mut self.slots[entry.slot];
                slot.pages = slot.pages.saturating_sub(1);
            }
        }
    }

    /// Let go of a session. A second call (or a call after a reap or a crash)
    /// returns `None` and changes nothing.
    pub fn release(&mut self, key: &str) -> Option<Released> {
        let entry = self.sessions.remove(key)?;
        let slot = &mut self.slots[entry.slot];
        slot.sessions = slot.sessions.saturating_sub(1);
        slot.pages = slot.pages.saturating_sub(entry.pages);
        Some(Released {
            slot: entry.slot,
            pages: entry.pages,
        })
    }

    /// The session did something.
    pub fn touch(&mut self, key: &str, now: Instant) {
        if let Some(entry) = self.sessions.get_mut(key) {
            entry.last_touch = now;
            entry.reap_reported = false;
        }
    }

    /// Sessions past their idle or age limit. With `enforce` they are removed
    /// and tombstoned; without it (log mode) nothing changes except that each
    /// candidate is reported once until it is touched again.
    pub fn reap_idle(&mut self, now: Instant, enforce: bool) -> Vec<Reaped> {
        self.prune_tombstones(now);
        let mut due: Vec<Reaped> = self
            .sessions
            .iter()
            .filter(|(_, entry)| enforce || !entry.reap_reported)
            .filter_map(|(key, entry)| {
                let reason = if now.saturating_duration_since(entry.admitted_at) >= entry.age_limit
                {
                    ReapReason::MaxAge
                } else if now.saturating_duration_since(entry.last_touch) >= entry.idle_limit {
                    ReapReason::Idle
                } else {
                    return None;
                };
                Some(Reaped {
                    session_key: key.clone(),
                    slot: entry.slot,
                    reason,
                    pages: entry.pages,
                })
            })
            .collect();
        due.sort_by(|a, b| a.session_key.cmp(&b.session_key));
        for reaped in &due {
            if enforce {
                self.release(&reaped.session_key);
                self.tombstones.insert(
                    reaped.session_key.clone(),
                    (now, LossReason::Reaped(reaped.reason)),
                );
            } else if let Some(entry) = self.sessions.get_mut(&reaped.session_key) {
                entry.reap_reported = true;
            }
        }
        due
    }

    /// Retiring slots with nothing left on them: close them.
    pub fn retire_candidates(&self) -> Vec<SlotId> {
        self.slots
            .iter()
            .filter(|slot| slot.status == SlotStatus::Retiring && slot.sessions == 0)
            .map(|slot| slot.id)
            .collect()
    }

    /// Stop admitting to `slot`; it closes once empty.
    pub fn retire(&mut self, slot: SlotId) {
        if let Some(state) = self.slots.get_mut(slot) {
            state.status = SlotStatus::Retiring;
        }
    }

    /// The browser of `slot` at `generation` is gone. Every session on it is
    /// lost and tombstoned; the keys come back so the caller can drop their
    /// pages. A notice for an older generation (the slot has been relaunched
    /// since) or a slot already dead changes nothing.
    pub fn mark_dead(&mut self, slot: SlotId, generation: u64, now: Instant) -> Vec<String> {
        let Some(state) = self.slots.get_mut(slot) else {
            return Vec::new();
        };
        if state.generation != generation || state.status == SlotStatus::Dead || generation == 0 {
            return Vec::new();
        }
        state.status = SlotStatus::Dead;
        state.healthy = Some(false);
        let mut lost: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, entry)| entry.slot == slot)
            .map(|(key, _)| key.clone())
            .collect();
        lost.sort();
        for key in &lost {
            self.release(key);
            self.tombstones
                .insert(key.clone(), (now, LossReason::ProcessLost));
        }
        lost
    }

    /// Why `key` is gone, if it was lost in the last ten minutes.
    pub fn loss(&self, key: &str, now: Instant) -> Option<LossReason> {
        self.tombstones
            .get(key)
            .filter(|(at, _)| now.saturating_duration_since(*at) < TOMBSTONE_TTL)
            .map(|(_, reason)| *reason)
    }

    /// Record a probe of `slot` at `generation`. Returns true when this probe
    /// made the slot unhealthy.
    pub fn record_probe(&mut self, slot: SlotId, generation: u64, ok: bool) -> bool {
        let Some(state) = self.slots.get_mut(slot) else {
            return false;
        };
        if state.generation != generation || state.status == SlotStatus::Dead {
            return false;
        }
        if ok {
            state.probe_failures = 0;
            state.healthy = Some(true);
            false
        } else {
            state.probe_failures = state.probe_failures.saturating_add(1);
            let was_unhealthy = state.healthy == Some(false);
            if state.probe_failures >= PROBE_FAILURES_UNHEALTHY {
                state.healthy = Some(false);
            }
            !was_unhealthy && state.healthy == Some(false)
        }
    }

    fn prune_tombstones(&mut self, now: Instant) {
        self.tombstones
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < TOMBSTONE_TTL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(slots: usize, max_sessions: usize, max_pages: usize) -> SlotConfig {
        SlotConfig {
            slots,
            max_sessions,
            max_pages_per_slot: max_pages,
            session_idle: Duration::from_secs(100),
            session_max_age: Duration::from_secs(1_000),
        }
    }

    const TTL: Duration = Duration::from_secs(10);

    fn slot_of(admission: Admission) -> SlotId {
        match admission {
            Admission::Admitted { slot, .. } => slot,
            Admission::Busy { .. } => panic!("expected admission, got {admission:?}"),
        }
    }

    #[test]
    fn admit_fills_least_loaded_slot_first() {
        let now = Instant::now();
        let mut book = SlotBook::new(cfg(3, 0, 0));
        for slot in 0..3 {
            book.launched(slot);
        }
        let first: Vec<SlotId> = (0..3)
            .map(|i| slot_of(book.admit(&format!("s{i}"), TTL, now)))
            .collect();
        assert_eq!(first, vec![0, 1, 2], "one per slot before doubling up");
        book.release("s1");
        assert_eq!(slot_of(book.admit("s3", TTL, now)), 1, "the emptied slot first");
        // A ready slot is preferred over a dead one at the same load.
        let mut book = SlotBook::new(cfg(2, 0, 0));
        book.launched(1);
        assert_eq!(slot_of(book.admit("a", TTL, now)), 1);
        // The same key twice is one session.
        let again = book.admit("a", TTL, now);
        assert!(matches!(again, Admission::Admitted { fresh: false, .. }));
        assert_eq!(book.sessions_open(), 1);
    }

    #[test]
    fn admit_returns_busy_at_max_sessions_with_retry_after() {
        let now = Instant::now();
        let mut book = SlotBook::new(cfg(1, 2, 0));
        let first = book.admit("a", TTL, now);
        assert!(matches!(
            first,
            Admission::Admitted {
                slot: 0,
                needs_launch: true,
                fresh: true
            }
        ));
        book.launched(0);
        assert!(matches!(
            book.admit("b", TTL, now),
            Admission::Admitted {
                needs_launch: false,
                ..
            }
        ));
        assert_eq!(
            book.admit("c", TTL, now),
            Admission::Busy {
                limit: 2,
                active: 2,
                retry_after_s: 15
            }
        );
        // An admitted key is not refused by the cap.
        assert!(matches!(
            book.admit("a", TTL, now),
            Admission::Admitted { fresh: false, .. }
        ));
        book.release("a");
        assert!(matches!(
            book.admit("c", TTL, now),
            Admission::Admitted { fresh: true, .. }
        ));
        // 0 is no cap.
        let mut open = SlotBook::new(cfg(1, 0, 0));
        for i in 0..500 {
            assert!(matches!(
                open.admit(&format!("k{i}"), TTL, now),
                Admission::Admitted { .. }
            ));
        }
    }

    #[test]
    fn page_cap_is_per_slot() {
        let now = Instant::now();
        let mut book = SlotBook::new(cfg(2, 0, 2));
        book.launched(0);
        book.launched(1);
        assert_eq!(slot_of(book.admit("a", TTL, now)), 0);
        assert_eq!(slot_of(book.admit("b", TTL, now)), 1);
        assert_eq!(slot_of(book.admit("c", TTL, now)), 0);
        assert_eq!(book.page_opened("a", now), Ok(0));
        assert_eq!(book.page_opened("c", now), Ok(0));
        // Slot 0 is full, whichever of its sessions asks.
        assert_eq!(
            book.page_opened("a", now),
            Err(PageRefusal::Cap {
                limit: 2,
                active: 2
            })
        );
        // Slot 1 is not.
        assert_eq!(book.page_opened("b", now), Ok(1));
        assert_eq!(book.page_opened("b", now), Ok(1));
        book.page_closed("c");
        assert_eq!(book.page_opened("a", now), Ok(0));
        // A released session takes its pages with it.
        book.release("a");
        assert_eq!(book.slot(0).unwrap().pages, 0);
        assert_eq!(
            book.page_opened("a", now),
            Err(PageRefusal::NotAdmitted(None))
        );
    }

    #[test]
    fn reap_idle_returns_only_sessions_past_threshold_and_respects_touch() {
        let t0 = Instant::now();
        let mut book = SlotBook::new(cfg(1, 0, 0));
        book.launched(0);
        book.admit("idle", TTL, t0);
        book.admit("busy", TTL, t0);
        book.page_opened("idle", t0).unwrap();
        book.touch("busy", t0 + Duration::from_secs(90));
        let t1 = t0 + Duration::from_secs(120);

        // Log mode: reported once, nothing removed.
        let logged = book.reap_idle(t1, false);
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].session_key, "idle");
        assert_eq!(logged[0].reason, ReapReason::Idle);
        assert_eq!(logged[0].pages, 1);
        assert!(book.reap_idle(t1, false).is_empty(), "reported once");
        assert_eq!(book.sessions_open(), 2);

        // Enforced: only the idle one goes, and it is tombstoned.
        let reaped = book.reap_idle(t1, true);
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0].session_key, "idle");
        assert!(book.is_admitted("busy"));
        assert!(!book.is_admitted("idle"));
        assert_eq!(book.slot(0).unwrap().pages, 0);
        assert_eq!(
            book.loss("idle", t1),
            Some(LossReason::Reaped(ReapReason::Idle))
        );
        assert_eq!(book.loss("idle", t1 + TOMBSTONE_TTL), None, "forgotten");
        // A touch resets the clock.
        book.touch("busy", t1);
        assert!(book.reap_idle(t1 + Duration::from_secs(99), true).is_empty());
    }

    #[test]
    fn reap_uses_max_of_two_ttl_and_idle_floor() {
        let t0 = Instant::now();
        let mut book = SlotBook::new(cfg(1, 0, 0));
        book.launched(0);
        // 2 x 10 s < the 100 s floor: the floor wins.
        book.admit("short", Duration::from_secs(10), t0);
        // 2 x 80 s = 160 s > 100 s: twice the lease TTL wins.
        book.admit("long", Duration::from_secs(80), t0);
        let at_100 = book.reap_idle(t0 + Duration::from_secs(100), true);
        assert_eq!(
            at_100
                .iter()
                .map(|r| r.session_key.as_str())
                .collect::<Vec<_>>(),
            vec!["short"]
        );
        assert!(book
            .reap_idle(t0 + Duration::from_secs(159), true)
            .is_empty());
        let at_160 = book.reap_idle(t0 + Duration::from_secs(160), true);
        assert_eq!(at_160[0].session_key, "long");

        // Max age applies however busy a session is, with the same 2 x TTL rule.
        book.admit("old", Duration::from_secs(10), t0);
        let mut now = t0;
        while now < t0 + Duration::from_secs(999) {
            now += Duration::from_secs(50);
            book.touch("old", now);
            if now < t0 + Duration::from_secs(1_000) {
                assert!(book.reap_idle(now, true).is_empty(), "{now:?}");
            }
        }
        let aged = book.reap_idle(t0 + Duration::from_secs(1_000), true);
        assert_eq!(aged[0].reason, ReapReason::MaxAge);
    }

    #[test]
    fn mark_dead_returns_affected_sessions_and_blocks_new_admission_until_relaunch() {
        let now = Instant::now();
        let mut book = SlotBook::new(cfg(1, 0, 0));
        book.admit("a", TTL, now);
        let generation = book.launched(0);
        book.admit("b", TTL, now);
        book.page_opened("a", now).unwrap();

        // A notice about a generation that is not running changes nothing.
        assert!(book.mark_dead(0, generation + 1, now).is_empty());
        let lost = book.mark_dead(0, generation, now);
        assert_eq!(lost, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(book.slot(0).unwrap().status, SlotStatus::Dead);
        assert_eq!(book.slot(0).unwrap().sessions, 0);
        assert_eq!(book.slot(0).unwrap().pages, 0);
        assert_eq!(book.loss("a", now), Some(LossReason::ProcessLost));
        assert!(book.mark_dead(0, generation, now).is_empty(), "once");
        // The lost session cannot open a page on the dead browser.
        assert_eq!(
            book.page_opened("a", now),
            Err(PageRefusal::NotAdmitted(Some(LossReason::ProcessLost)))
        );
        // A new session is admitted only together with a relaunch.
        assert!(matches!(
            book.admit("c", TTL, now),
            Admission::Admitted {
                needs_launch: true,
                ..
            }
        ));
        assert_eq!(book.launched(0), generation + 1);
        assert!(matches!(
            book.admit("d", TTL, now),
            Admission::Admitted {
                needs_launch: false,
                ..
            }
        ));
        // A stale notice for the old generation leaves the new one alone.
        assert!(book.mark_dead(0, generation, now).is_empty());
        assert_eq!(book.slot(0).unwrap().status, SlotStatus::Ready);
        // Admitting the lost key again starts it fresh.
        book.admit("a", TTL, now);
        assert_eq!(book.loss("a", now), None);
    }

    #[test]
    fn release_is_idempotent() {
        let now = Instant::now();
        let mut book = SlotBook::new(cfg(1, 1, 0));
        book.launched(0);
        book.admit("a", TTL, now);
        book.page_opened("a", now).unwrap();
        assert_eq!(book.release("a"), Some(Released { slot: 0, pages: 1 }));
        assert_eq!(book.release("a"), None);
        assert_eq!(book.slot(0).unwrap().sessions, 0);
        assert_eq!(book.slot(0).unwrap().pages, 0);
        book.page_closed("a");
        assert_eq!(book.slot(0).unwrap().pages, 0);
        // The cap counted it once and freed it once.
        assert!(matches!(
            book.admit("b", TTL, now),
            Admission::Admitted { .. }
        ));
    }

    #[test]
    fn probe_failures_mark_unhealthy_after_two_and_recover() {
        let mut book = SlotBook::new(cfg(1, 0, 0));
        let generation = book.launched(0);
        assert!(!book.record_probe(0, generation, false));
        assert_eq!(book.slot(0).unwrap().healthy, None);
        assert!(book.record_probe(0, generation, false), "second failure");
        assert!(!book.record_probe(0, generation, false), "reported once");
        assert_eq!(book.slot(0).unwrap().healthy, Some(false));
        assert!(!book.record_probe(0, generation, true));
        assert_eq!(book.slot(0).unwrap().healthy, Some(true));
        // A probe of an old generation is ignored.
        assert!(!book.record_probe(0, generation + 5, false));
    }

    #[test]
    fn retire_candidates_are_empty_retiring_slots() {
        let now = Instant::now();
        let mut book = SlotBook::new(cfg(2, 0, 0));
        book.launched(0);
        book.launched(1);
        book.admit("a", TTL, now);
        book.retire(0);
        assert!(book.retire_candidates().is_empty(), "still has a session");
        assert_eq!(slot_of(book.admit("b", TTL, now)), 1, "no new sessions");
        book.release("a");
        assert_eq!(book.retire_candidates(), vec![0]);
    }

    #[test]
    fn config_from_env_clamps_values() {
        let defaults = BrowserPoolConfig::from_lookup(|_| None);
        assert_eq!(defaults, BrowserPoolConfig::default());
        assert_eq!(defaults.max_sessions, 40);
        assert_eq!(defaults.max_pages_per_slot, 24);
        assert_eq!(defaults.session_idle, Duration::from_secs(3_600));
        assert_eq!(defaults.session_max_age, Duration::from_secs(21_600));
        assert_eq!(defaults.reaper_interval, Duration::from_secs(30));
        assert_eq!(defaults.probe_interval, Duration::from_secs(15));
        assert_eq!(defaults.reaper, ReaperMode::Log);

        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.to_string())
            }
        };
        let clamped = BrowserPoolConfig::from_lookup(env(&[
            ("QUARRY_BROWSER_MAX_SESSIONS", "999999"),
            ("QUARRY_BROWSER_MAX_PAGES_PER_SLOT", "0"),
            ("QUARRY_BROWSER_SESSION_IDLE_S", "1"),
            ("QUARRY_BROWSER_SESSION_MAX_AGE_S", "5"),
            ("QUARRY_BROWSER_REAPER_INTERVAL_S", "0"),
            ("QUARRY_BROWSER_PROBE_INTERVAL_S", "99999"),
            ("QUARRY_BROWSER_REAPER", "1"),
        ]));
        assert_eq!(clamped.max_sessions, 4_096);
        assert_eq!(clamped.max_pages_per_slot, 0, "0 disables the cap");
        assert_eq!(clamped.session_idle, Duration::from_secs(30));
        assert_eq!(clamped.session_max_age, Duration::from_secs(60));
        assert_eq!(clamped.reaper_interval, Duration::from_secs(1));
        assert_eq!(clamped.probe_interval, Duration::from_secs(3_600));
        assert_eq!(clamped.reaper, ReaperMode::On);

        // Age never below idle.
        let ordered = BrowserPoolConfig::from_lookup(env(&[
            ("QUARRY_BROWSER_SESSION_IDLE_S", "7200"),
            ("QUARRY_BROWSER_SESSION_MAX_AGE_S", "600"),
        ]));
        assert_eq!(ordered.session_max_age, Duration::from_secs(7_200));

        // Garbage is the default.
        let garbage = BrowserPoolConfig::from_lookup(env(&[
            ("QUARRY_BROWSER_MAX_SESSIONS", "lots"),
            ("QUARRY_BROWSER_REAPER", "sometimes"),
        ]));
        assert_eq!(garbage.max_sessions, 40);
        assert_eq!(garbage.reaper, ReaperMode::Log);
        let off = BrowserPoolConfig::from_lookup(env(&[("QUARRY_BROWSER_REAPER", "0")]));
        assert_eq!(off.reaper, ReaperMode::Off);
    }
}
