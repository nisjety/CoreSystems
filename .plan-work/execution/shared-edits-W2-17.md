## WS24-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/lib.rs: add the line `pub mod owner_actions;` in alphabetical position among the `pub mod` declarations (next to `pub mod ticket_tools;` at about line 51). Until it lands, execution-core does not compile and the Rust tests cannot run.

## WS24-S6
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: in the `if modelActionViewVerifier, viewVerifierErr := modelActionViewVerifierFromEnv(os.Getenv); viewVerifierErr != nil {...} else {` branch (about lines 267-270), before `ch = ch.WithModelActionViewVerifier(modelActionViewVerifier)` insert: `if admissionStore, storeErr := registry.NewAgentActionAdmissionsStore(pool); storeErr == nil { modelActionViewVerifier.WithAgentActionAdmissions(admissionStore, api.ModelActionAdmissionTableFromLookup(os.Getenv)) }` (add the registry import if absent). Without it the table can never be on, which is the safe default.

