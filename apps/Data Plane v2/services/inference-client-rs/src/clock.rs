//! The one place the client reads the time.
//!
//! Bearer validation and the cache both compare the token's claims with "now".
//! Reading the clock through a trait lets a contract test replay tokens that a
//! real Auth Core mint captured on a past date (their `exp` is long gone on the
//! wall clock) and move time forward to prove a refresh, instead of sleeping or
//! patching the clock of the whole process.

use chrono::{DateTime, Utc};

/// A source of the current UTC time.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

/// The wall clock. What every service uses.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
