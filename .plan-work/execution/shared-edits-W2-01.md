## WS21-S0
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: add a row to the table whose header is 'Capability | Canonical Owner | Storage | Relay / Enforcement | Async | Duplication verdict' (place it after the Watches row): '| Browser site policy and Do grant derivation (WS21, DEC-19/20) | **Control org-core** owns the site record; **Model Plane browser-broker** mints the grant from the verified principal; execution-core only pre-resolves ask or deny | Postgres in org-core (planned, WS21-S3); broker grants in memory until QRY picks the store | Quarry ValidateGrant and the domain fence enforce every action | consent cards via WS13 cause browser_site_consent (planned) | one owner per record; grant derivation vs grant store = QRY (DEC-20); not built yet |'.

## WS21-S0
shared-edit apps/master-ownership-matrix.md: add a row to the table with header 'Tool / feature | Quarry v2 | Data Plane | Model Plane | App Shell | Rust | Go | Python' (place it after the 'Browser actions' row): '| Browser site policy (block / read / act, per user and per org) | **Enforcer** (domain fence, final check) | — | **Owner of grant mint** (browser-broker, from the verified principal); policy of record is Control org-core, not a column | — | `quarry-browser` fence | `browser-broker` mint; org-core record (planned WS21-S3) | — |'.

## WS23-S0
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: add one row for connected-app reads (read_provider_action, WS23): execution-core integration_tools.rs builds the call; integration-corev2 owns connections and the operation catalogue; model-gateway Ask path is WS23 S5. Match the existing table columns.

