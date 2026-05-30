package orchestration

import "github.com/triodelab/model-plane/pkg/envelope"

// IdemPrefix returns blake3("orchestrator-core|<event>|<thread>|<request>") as
// lowercase hex. Cross-language parity with the Rust and Python implementations
// is enforced by pkg/envelope.TestDeriveIdempotencyHash_MatchesRust.
func IdemPrefix(event, thread, request string) string {
	return envelope.DeriveIdempotencyHash(Producer, event, thread, request)
}
