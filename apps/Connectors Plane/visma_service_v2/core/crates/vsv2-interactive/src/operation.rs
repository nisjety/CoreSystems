use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use vsv2_common::TenantId;

use crate::error::{InteractiveError, Result};
use crate::request::WriteRequest;
use crate::transport::Reader;

/// What an operation turns validated input into.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub request: WriteRequest,
    /// One line for the approver: what will change.
    pub summary: String,
}

/// A named, typed write workflow (e.g. `salesorder.update_line`). Implementations validate their
/// own input, read whatever precondition they need (the ETag) through `reader`, and return the
/// exact request. They never send anything; only the [`crate::Executor`] does.
#[async_trait]
pub trait Operation: Send + Sync {
    fn name(&self) -> &'static str;

    async fn build(&self, tenant: &TenantId, reader: &dyn Reader, input: &Value) -> Result<Built>;
}

#[derive(Default, Clone)]
pub struct OperationRegistry {
    ops: HashMap<&'static str, Arc<dyn Operation>>,
}

impl OperationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, op: impl Operation + 'static) -> Self {
        self.ops.insert(op.name(), Arc::new(op));
        self
    }

    pub fn get(&self, name: &str) -> Result<&Arc<dyn Operation>> {
        self.ops
            .get(name)
            .ok_or_else(|| InteractiveError::UnknownOperation(name.to_string()))
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.ops.keys().copied().collect();
        v.sort_unstable();
        v
    }
}
