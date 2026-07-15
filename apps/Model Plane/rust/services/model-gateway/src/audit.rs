//! Inline tool data classification used by the Gateway's Session Core
//! reserve-before-execute/finalize-after-execute contract. Session Core owns
//! the transactional audit outbox and is the sole Audit Core producer.

/// Conservative data-category classification for an inline builtin tool. The
/// governed path's per-tool classification is authoritative; unknown tools are
/// explicitly unclassified rather than assigned a fabricated category.
#[must_use]
pub fn tool_data_category(tool: &str) -> &'static str {
    match tool {
        "web_search" | "fetch_url" | "web_fetch" | "news" | "traffic" | "yr_weather"
        | "company_lookup" | "track_shipment" => "public_non_personal",
        "knowledge_search" => "customer_private",
        _ => "unclassified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_category_maps_known_tools_and_defaults_unclassified() {
        assert_eq!(tool_data_category("company_lookup"), "public_non_personal");
        assert_eq!(tool_data_category("web_search"), "public_non_personal");
        assert_eq!(tool_data_category("knowledge_search"), "customer_private");
        assert_eq!(tool_data_category("some_mcp_tool"), "unclassified");
    }
}
