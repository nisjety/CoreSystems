# Learning Plane instructions

- Own human-learning domain records: course structure/revisions, assignments, attempts, formative progress and compatibility validation. AI memory/skill extraction remains Model Plane-owned.
- Use Go for the service and durable workflows. Start with one modular service. No new auth, billing, model-serving or retrieval implementation.
- Control owns identity, organization membership, capability authority and audit. Classroom authorization must narrow Control authority; a teacher role alone never grants device control.
- Data owns source materials/assets, source versions and retrieval. Keep references to those records, not a second document/embedding store.
- Model owns reasoning, speech provider execution and agent runs. Self-hosted recognition, translation and speech synthesis are different capabilities.
- Default to strict self-hosted processing. An external provider, including a ChatGPT subscription, is incompatible with that profile. Never silently fall back to an external provider.
- Application owns realtime projections and notifications. Learning owns the source of truth for learning attempts and teaching-assistance intent; Control owns canonical audit ingestion.
- A policy function is not an authentication mechanism. Never accept client-supplied actor/org/grant evidence as verified authority.
- Preserve typed data/error envelopes. School/course operations require authenticated Control authority and durable Learning storage; device actions stay fail-closed until device-side enforcement exists. Do not fabricate users, successful sessions or progress.
- Native actions must be checked again on the device at execution time. Screen-view permission does not grant input or AI-processing permission. Browser actions still execute through Quarry.
- Run `go test ./...` and `go vet ./...` in learning-core for backend changes. Keep implementation status and contracts honest.
