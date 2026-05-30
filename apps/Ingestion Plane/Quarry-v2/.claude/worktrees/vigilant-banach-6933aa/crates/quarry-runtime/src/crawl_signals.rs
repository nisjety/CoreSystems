//! Pause / resume / cancel signal channel for live crawls.
//!
//! The Go orchestrator (Temporal) is the durable owner of crawl state, but
//! Rust runs the hot loop. When operators send a pause signal via the
//! orchestrator, the orchestrator forwards it through `CrawlSignals` so the
//! frontier loop pauses at the next safe point (after the current page) and
//! checkpoints to Temporal.
//!
//! ## Lifecycle
//!
//! ```text
//!     [ Running ]
//!         │  pause()
//!         ▼
//!     [ Paused ]──┐
//!         │  resume()    cancel()
//!         ▼              ▼
//!     [ Running ]    [ Cancelled ]
//! ```
//!
//! Cancelled is terminal; resume() is a no-op once cancelled.
//!
//! Built on a tokio watch channel so loops can `.await` on signal changes
//! cheaply without polling.

use std::sync::Arc;

use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlSignal {
    Running,
    Paused,
    Cancelled,
}

/// Producer-side handle: clone freely, send signals to all consumers.
#[derive(Debug, Clone)]
pub struct CrawlSignalsProducer {
    tx: Arc<watch::Sender<CrawlSignal>>,
}

impl CrawlSignalsProducer {
    pub fn pause(&self) {
        // ignore "no receivers" error — the loop may have terminated already
        let _ = self.tx.send(CrawlSignal::Paused);
    }

    pub fn resume(&self) {
        if *self.tx.borrow() == CrawlSignal::Cancelled {
            return; // cancellation is terminal
        }
        let _ = self.tx.send(CrawlSignal::Running);
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(CrawlSignal::Cancelled);
    }

    pub fn current(&self) -> CrawlSignal {
        *self.tx.borrow()
    }
}

/// Consumer-side handle held by the crawl loop.
#[derive(Debug, Clone)]
pub struct CrawlSignalsConsumer {
    rx: watch::Receiver<CrawlSignal>,
}

impl CrawlSignalsConsumer {
    pub fn current(&self) -> CrawlSignal {
        *self.rx.borrow()
    }

    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow() == CrawlSignal::Cancelled
    }

    /// Block until the loop should run (signal flips to `Running`).
    /// Returns `false` if the producer was dropped or the crawl was cancelled
    /// — the loop should stop.
    pub async fn wait_until_runnable(&mut self) -> bool {
        loop {
            // Snapshot the current value, then drop the borrow guard before awaiting.
            let current = *self.rx.borrow();
            match current {
                CrawlSignal::Running => return true,
                CrawlSignal::Cancelled => return false,
                CrawlSignal::Paused => {
                    if self.rx.changed().await.is_err() {
                        return false; // producer dropped
                    }
                }
            }
        }
    }
}

pub fn pair() -> (CrawlSignalsProducer, CrawlSignalsConsumer) {
    let (tx, rx) = watch::channel(CrawlSignal::Running);
    (
        CrawlSignalsProducer { tx: Arc::new(tx) },
        CrawlSignalsConsumer { rx },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn default_state_is_running() {
        let (_p, c) = pair();
        assert_eq!(c.current(), CrawlSignal::Running);
        assert!(!c.is_cancelled());
    }

    #[tokio::test]
    async fn pause_then_resume_unblocks_consumer() {
        let (p, mut c) = pair();
        p.pause();
        assert_eq!(c.current(), CrawlSignal::Paused);

        let p2 = p.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            p2.resume();
        });
        let runnable = c.wait_until_runnable().await;
        assert!(runnable);
    }

    #[tokio::test]
    async fn cancel_returns_false_from_wait() {
        let (p, mut c) = pair();
        p.pause();
        let p2 = p.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            p2.cancel();
        });
        let runnable = c.wait_until_runnable().await;
        assert!(!runnable);
        assert!(c.is_cancelled());
    }

    #[tokio::test]
    async fn resume_after_cancel_is_noop() {
        let (p, c) = pair();
        p.cancel();
        p.resume();
        assert_eq!(c.current(), CrawlSignal::Cancelled);
    }

    #[tokio::test]
    async fn wait_returns_immediately_when_already_running() {
        let (_p, mut c) = pair();
        let runnable = c.wait_until_runnable().await;
        assert!(runnable);
    }

    #[tokio::test]
    async fn multi_subscriber_all_see_pause() {
        let (p, mut c1) = pair();
        let mut c2 = c1.clone();
        p.pause();
        // both consumers see paused
        assert_eq!(c1.current(), CrawlSignal::Paused);
        assert_eq!(c2.current(), CrawlSignal::Paused);

        // both unblock when resumed
        let p2 = p.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            p2.resume();
        });
        let r1 = c1.wait_until_runnable().await;
        let r2 = c2.wait_until_runnable().await;
        assert!(r1);
        assert!(r2);
    }
}
