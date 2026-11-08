import sys
sys.path.insert(0, r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\w314guard')
from edit import edit

edit('ZDR.md', [
("""(learning_events.rs), so the review never receives them.

### 5.6 What the waiver can never do
""", """(learning_events.rs), so the review never receives them.

Cited passages and the research stamp (EP-2-S4, behind MODEL_GATEWAY_CITATION_PASSAGES): a
citation may carry `quote` (the passage, at most 400 characters) and, for org knowledge,
`locator` (document_id and chunk_index); a deep-research report artifact may carry
`provenance` (question, sub-queries, read sources, model, coverage). All three stream live on
every turn and are stored in the assistant message metadata (citations, artifacts) only on a
persistent turn, erased with the message by the GDPR path. A ZDR or waived turn stores neither
(session_flow::persistable_citations / persistable_artifacts; goldens
e_zdr_turn_kb_citation_passage_streams_not_persisted, e_waived_research_metadata_not_persisted).
The viewer's "more" read is Data Plane's ACL'd chunk read and stores nothing.

### 5.6 What the waiver can never do
""", 1),
("""(feature `browser_human_assist`) and no approval row is written. | Built, read in source. |""",
 """(feature `browser_human_assist`) and no approval row is written. A wide-research run (`InvokeRequest.wide_research`) and `POST /v1/research/wide/estimate` are refused with `zdr_feature_not_supported`, feature `agentic` (EP-2-S8); the estimate stores nothing. The write-approval preview (EP-10-S1, owner-only, decrypted on read and never stored) adds no store and no feature key: a ZDR bearer is refused by the continuation gate with `zdr_durable_mutation_forbidden`, and a waived credential by session-core (waiver route mismatch). | Built, read in source. |""", 1),
])
