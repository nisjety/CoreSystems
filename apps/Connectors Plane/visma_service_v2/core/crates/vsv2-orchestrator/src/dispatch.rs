//! Source dispatch — single switch point for choosing which client runs an
//! endpoint for a given tenant. Phase 3.

use crate::adapter::FetchAdapter;
use crate::registry::EndpointDefinition;
use vsv2_common::{Error, Result, Source, TenantId};

pub struct DispatchTable {
    pub erp: Box<dyn FetchAdapter>,
    pub datamart: Box<dyn FetchAdapter>,
    pub expense: Box<dyn FetchAdapter>,
}

impl DispatchTable {
    pub fn pick(&self, def: &EndpointDefinition, tenant: &TenantId) -> Result<&dyn FetchAdapter> {
        self.pick_source(def.live_source(def.effective_source(tenant))?)
    }

    pub fn pick_source(&self, source: Source) -> Result<&dyn FetchAdapter> {
        match source {
            Source::ErpRest => Ok(self.erp.as_ref()),
            Source::DatamartGraphql => Ok(self.datamart.as_ref()),
            Source::ExpenseApi => Ok(self.expense.as_ref()),
        }
    }

    pub fn pick_fallback(&self, def: &EndpointDefinition) -> Result<&dyn FetchAdapter> {
        let src = def
            .datamart
            .as_ref()
            .map(|d| d.fallback_source)
            .unwrap_or(def.source);
        match src {
            Source::ErpRest => Ok(self.erp.as_ref()),
            Source::DatamartGraphql => Err(Error::Other(
                "fallback source must not be datamart_graphql".into(),
            )),
            Source::ExpenseApi => Ok(self.expense.as_ref()),
        }
    }
}
