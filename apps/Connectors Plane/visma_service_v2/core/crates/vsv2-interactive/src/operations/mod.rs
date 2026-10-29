//! Concrete Layer 3 write operations. One file per workflow.

mod order_read;
pub mod salesorder_create;
pub mod salesorder_patch;
pub mod salesorder_status;

pub use salesorder_create::SalesOrderCreate;
pub use salesorder_patch::SalesOrderPatch;
pub use salesorder_status::SalesOrderSetStatus;

use crate::operation::OperationRegistry;

/// Every operation this crate ships. Registered operations are still denied unless the
/// [`crate::WritePolicy`] allow-lists them.
pub fn default_registry() -> OperationRegistry {
    OperationRegistry::new()
        .with(SalesOrderPatch)
        .with(SalesOrderCreate)
        .with(SalesOrderSetStatus)
}
