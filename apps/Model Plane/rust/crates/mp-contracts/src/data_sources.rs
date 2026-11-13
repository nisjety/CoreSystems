//! The data sources a chat turn may draw on, as one reviewed table: the tools
//! that reach each source, when the source is the right one, what a call costs,
//! and whether a zero-retention turn may use it.
//!
//! The model gateway turns this table into a short catalogue for the prompt
//! (`retrieval_tools::data_source_catalogue_notice`), listing only the sources
//! whose tools the turn actually offers. The table holds no org data and no
//! secrets: it is the same for every org and every turn.
//!
//! The tool names are the gateway's own, typed here because this crate cannot
//! depend on the gateway. A tool renamed in the gateway must be renamed here in
//! the same change, or the catalogue stops listing that source.
//!
//! The catalogue is part of the prompt, so the order of [`DATA_SOURCES`] is part
//! of the contract: the sources are listed in this order and never sorted at run
//! time. A new source goes in a reviewed place in the table.

/// What a call to a source costs, as a hint for the model to weigh. It is not a
/// budget, and nothing enforces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostHint {
    /// No metered cost per call.
    Free,
    /// A metered call that is small: an internal retrieval or a bounded read.
    MeteredLow,
    /// A metered call that is large: a third-party query billed per request.
    MeteredHigh,
}

impl CostHint {
    /// The spelling the catalogue prints.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::MeteredLow => "metered_low",
            Self::MeteredHigh => "metered_high",
        }
    }
}

/// One source a turn may draw on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataSource {
    /// The source's name, as the catalogue lists it.
    pub id: &'static str,
    /// The gateway tools that reach this source. A turn offers the source when it
    /// offers any one of them.
    pub tool_names: &'static [&'static str],
    /// One sentence on when this source is the right one to reach for.
    pub use_when: &'static str,
    /// What a call costs.
    pub cost_hint: CostHint,
    /// Whether a zero-retention turn may use this source. Off for every source
    /// whose tools do not all carry the turn's zero-retention posture to the
    /// place the content is read from.
    pub zdr_allowed: bool,
}

/// The sources, in catalogue order.
pub const DATA_SOURCES: &[DataSource] = &[
    DataSource {
        id: "knowledge",
        tool_names: &["knowledge_search", "knowledge_graph_search"],
        use_when: "Search the organization's own ingested documents and knowledge graph before the public web when the answer may already be in them.",
        cost_hint: CostHint::MeteredLow,
        // Both tools send the turn's Data Plane retrieval mode, which is `ephemeral`
        // on a zero-retention turn. Contradiction and document-list reads send no
        // mode, so they are not catalogued here.
        zdr_allowed: true,
    },
    DataSource {
        id: "wiki",
        tool_names: &["knowledge_wiki_search"],
        use_when: "Look up a published wiki page when the answer is likely a curated how-to or policy page rather than one document.",
        cost_hint: CostHint::MeteredLow,
        // The wiki read sends no retrieval mode, so it does not carry a
        // zero-retention turn's posture to Data Plane.
        zdr_allowed: false,
    },
    DataSource {
        id: "web_search",
        tool_names: &["web_search"],
        use_when: "Search the public web for current or external facts that the organization's own sources do not hold.",
        cost_hint: CostHint::MeteredHigh,
        // Browsing is refused on a zero-retention invoke (`browse_web`).
        zdr_allowed: false,
    },
    DataSource {
        id: "web_fetch",
        tool_names: &["fetch_url", "read_page", "find_in_page", "extract_structured"],
        use_when: "Read a public web page the person named, or one a search returned, when a snippet is not enough.",
        cost_hint: CostHint::MeteredLow,
        // Browsing is refused on a zero-retention invoke (`browse_web`), and a
        // fetched page cannot be parked on a zero-retention turn.
        zdr_allowed: false,
    },
    DataSource {
        id: "connector_reads",
        tool_names: &["read_provider_action"],
        use_when: "Read the person's connected mail, calendar or files only when they ask about that material.",
        cost_hint: CostHint::MeteredLow,
        // A connector scope is refused on a zero-retention invoke (`connector_read`).
        zdr_allowed: false,
    },
    DataSource {
        id: "thread_reference",
        tool_names: &["thread_outline", "thread_search", "thread_read"],
        use_when: "Read an earlier conversation the person attached to this message, and only that conversation.",
        cost_hint: CostHint::Free,
        // A zero-retention turn is offered no thread reference tool.
        zdr_allowed: false,
    },
];
