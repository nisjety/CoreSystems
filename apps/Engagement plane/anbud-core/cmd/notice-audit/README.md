# Held-notice coverage audit

Run `go run ./cmd/notice-audit < notices.ndjson` from the anbud-core module.
Input is one JSON-encoded `doffin.Notice` per line (`ID`, `Body`; Go JSON encodes
Body as base64). Keep source exports private and delete them after the audit.
The command emits only aggregate counts: no notice identifiers, contact fields,
source bodies, descriptions, or winner names.

Counts describe the fixed input snapshot, not a live catalogue total. Doffin/TED
twins in an export are separate input records; the public catalogue can deduplicate
them. A parsed record means the notice shape is supported, not that every field is
present. Field counts indicate at least one parsed value of that kind. Document
links do not mean the linked files have been downloaded or read.
