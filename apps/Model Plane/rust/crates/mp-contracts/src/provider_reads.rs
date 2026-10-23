//! The provider read operations the agent may call, as the Rust side of the
//! contract.
//!
//! integration-corev2 owns the executor, and its Go test checks the golden in
//! `apps/Ingestion Plane/integration-corev2/internal/actions/testdata/`
//! against the executor's switch and the capability table. The fixture in
//! `tests/fixtures/provider-read-operations.json` is a byte-identical copy of
//! that golden. This table is what the fixture must say, and execution-core's
//! catalogue must list exactly these reads for the providers named here.
//!
//! Writes are not in this table. They stay in execution-core's catalogue as
//! approval-gated operations, and the golden lists them under `excludedWrites`.

/// One read operation of a connected provider, as the executor serves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderReadOperation {
    pub provider: &'static str,
    pub operation: &'static str,
    /// Parameter names the executor reads for the operation.
    pub params: &'static [&'static str],
    /// Capability the connection must hold, as requiredCapabilityForOperation names it.
    pub capability: &'static str,
    /// Sensitivity as requiredCapabilityForOperation classifies it.
    pub sensitive: bool,
}

pub const READ_OPERATIONS: &[ProviderReadOperation] = &[
    ProviderReadOperation {
        provider: "google",
        operation: "calendar.events",
        params: &["since", "until", "query", "maxResults", "pageToken"],
        capability: "calendar.read",
        sensitive: true,
    },
    ProviderReadOperation {
        provider: "google",
        operation: "drive.file.content",
        params: &["fileId", "mimeType", "exportAs"],
        capability: "drive.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "google",
        operation: "drive.files",
        params: &[
            "query",
            "match",
            "folderId",
            "driveId",
            "maxResults",
            "pageToken",
        ],
        capability: "drive.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "google",
        operation: "gmail.messages",
        params: &[
            "query",
            "from",
            "since",
            "until",
            "folder",
            "unread",
            "headers",
            "maxResults",
            "pageToken",
        ],
        capability: "gmail.read",
        sensitive: true,
    },
    ProviderReadOperation {
        provider: "google",
        operation: "gmail.messages.get",
        params: &["id"],
        capability: "gmail.read",
        sensitive: true,
    },
    ProviderReadOperation {
        provider: "google",
        operation: "profile",
        params: &[],
        capability: "profile.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "calendar.events",
        params: &["since", "until", "query", "maxResults", "pageToken"],
        capability: "calendar.read",
        sensitive: true,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "drive.file.content",
        params: &["itemId", "driveId"],
        capability: "sharepoint.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "drive.files",
        params: &["query", "folderId", "driveId", "maxResults", "pageToken"],
        capability: "sharepoint.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "mail.messages",
        params: &[
            "query",
            "from",
            "since",
            "until",
            "folder",
            "unread",
            "maxResults",
            "pageToken",
        ],
        capability: "mail.read",
        sensitive: true,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "mail.messages.get",
        params: &["messageId"],
        capability: "mail.read",
        sensitive: true,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "profile",
        params: &[],
        capability: "profile.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "sharepoint.drives",
        params: &["siteId", "pageToken"],
        capability: "sharepoint.read",
        sensitive: false,
    },
    ProviderReadOperation {
        provider: "microsoft",
        operation: "sharepoint.sites",
        params: &["query", "pageToken"],
        capability: "sharepoint.read",
        sensitive: false,
    },
];
