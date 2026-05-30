//! DriverRegistry — holds all available drivers keyed by DriverKind.
//!
//! Construct at startup, share via Arc across request handlers.
//! Per-request: call `build_driver(plan)` to get a FallbackDriver
//! that tries the plan's primary then its fallback chain.

use std::collections::HashMap;
use std::sync::Arc;

use quarry_core::output::DriverKind;

use crate::driver::Driver;
use crate::driver_plan::DriverPlan;
use crate::fallback_driver::FallbackDriver;

#[derive(Clone)]
pub struct DriverRegistry {
    drivers: HashMap<DriverKind, Arc<dyn Driver>>,
    default_kind: DriverKind,
}

impl DriverRegistry {
    pub fn new(default_kind: DriverKind) -> Self {
        Self {
            drivers: HashMap::new(),
            default_kind,
        }
    }

    pub fn register(&mut self, driver: Arc<dyn Driver>) {
        self.drivers.insert(driver.kind(), driver);
    }

    pub fn build_driver(&self, plan: &DriverPlan) -> Arc<dyn Driver> {
        if self.drivers.len() <= 1 {
            if let Some(d) = self
                .drivers
                .get(&plan.driver)
                .or_else(|| self.drivers.get(&self.default_kind))
            {
                return d.clone();
            }
        }
        Arc::new(FallbackDriver::from_plan(plan, self.drivers.clone()))
    }

    pub fn default_driver(&self) -> Option<Arc<dyn Driver>> {
        self.drivers.get(&self.default_kind).cloned()
    }

    pub fn has(&self, kind: DriverKind) -> bool {
        self.drivers.contains_key(&kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::FetchResponse;
    use async_trait::async_trait;
    use quarry_core::QuarryResult;
    use url::Url;

    struct StubDriver(DriverKind);

    #[async_trait]
    impl Driver for StubDriver {
        fn kind(&self) -> DriverKind {
            self.0
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: format!("from {:?}", self.0).into_bytes(),
                duration_ms: 1,
            })
        }
    }

    #[test]
    fn register_and_has() {
        let mut reg = DriverRegistry::new(DriverKind::Static);
        assert!(!reg.has(DriverKind::Static));
        reg.register(Arc::new(StubDriver(DriverKind::Static)));
        assert!(reg.has(DriverKind::Static));
        assert!(!reg.has(DriverKind::Tls));
    }

    #[test]
    fn default_driver_returns_registered() {
        let mut reg = DriverRegistry::new(DriverKind::Static);
        reg.register(Arc::new(StubDriver(DriverKind::Static)));
        assert!(reg.default_driver().is_some());
    }

    #[test]
    fn build_driver_single_returns_arc_directly() {
        let mut reg = DriverRegistry::new(DriverKind::Static);
        reg.register(Arc::new(StubDriver(DriverKind::Static)));
        let plan = DriverPlan::static_fetch("test");
        let driver = reg.build_driver(&plan);
        assert_eq!(driver.kind(), DriverKind::Static);
    }

    #[tokio::test]
    async fn build_driver_multi_returns_fallback() {
        let mut reg = DriverRegistry::new(DriverKind::Static);
        reg.register(Arc::new(StubDriver(DriverKind::Static)));
        reg.register(Arc::new(StubDriver(DriverKind::Tls)));
        let plan = DriverPlan::static_fetch("test");
        let driver = reg.build_driver(&plan);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = driver.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
