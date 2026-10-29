//! Where a source walk hands its records: one page at a time, as they arrive.

use serde_json::Value;

use crate::Result;

/// Receives the records of one source walk a page at a time, in the order the source returned
/// them. A walk never has to hold every record it fetched, so what it costs is one page, however
/// large the endpoint is. A sink that fails stops the walk.
pub trait PageSink: Send {
    fn accept(&mut self, page: Vec<Value>) -> Result<()>;
}

/// Collects every page, for a caller that wants the whole list at once.
impl PageSink for Vec<Value> {
    fn accept(&mut self, page: Vec<Value>) -> Result<()> {
        self.extend(page);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_vec_collects_the_pages_in_the_order_they_arrive() {
        let mut out: Vec<Value> = Vec::new();

        out.accept(vec![json!(1), json!(2)]).expect("first page");
        out.accept(vec![json!(3)]).expect("second page");

        assert_eq!(out, vec![json!(1), json!(2), json!(3)]);
    }
}
