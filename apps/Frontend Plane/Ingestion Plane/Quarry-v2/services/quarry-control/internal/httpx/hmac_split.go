// Package httpx - the orchestrator-only HMAC group (D2-050).
//
// Control's /v1/* routes were all verified with one shared secret, so a leaked
// edge secret could call the orchestrator's internal job routes, and finalize
// could end any org's job. The split: the routes only the orchestrator calls
// (everything under /v1/internal/jobs/ and /v1/internal/schedules/) are
// verified with QUARRY_CONTROL_INTERNAL_SECRET, every other route keeps the
// edge secret (QUARRY_INTERNAL_SECRET). A request signed with the wrong one of
// the two is refused with 401 on either side.
//
// POST /v1/internal/zdr-tombstones is NOT in the orchestrator group: its caller
// is the edge (crates/quarry-edge/src/zdr_tombstone.rs), which holds only the
// edge secret.

package httpx

import (
	"net/http"
	"strings"
)

// orchestratorOnlyPrefixes are the path prefixes whose callers are the
// orchestrator and nobody else. A prefix, not a list of routes, so a route added
// later under these prefixes is in the strict group from its first day.
var orchestratorOnlyPrefixes = []string{
	"/v1/internal/jobs/",
	"/v1/internal/schedules/",
}

// IsOrchestratorOnlyPath reports whether a request path belongs to the group
// verified with the control-internal secret.
func IsOrchestratorOnlyPath(path string) bool {
	for _, prefix := range orchestratorOnlyPrefixes {
		if strings.HasPrefix(path, prefix) {
			return true
		}
	}
	return false
}

// orchestratorOnlyRequest classifies by the decoded path. The decoded path is
// the stricter reading: an escaped spelling that chi would not route (it routes
// on the raw path) is classified as orchestrator-only here and answers 404 or 401,
// never reaches a handler through the weaker group.
func orchestratorOnlyRequest(r *http.Request) bool {
	return IsOrchestratorOnlyPath(r.URL.Path)
}

// SplitHMAC returns a middleware that verifies orchestrator-only paths with
// internal and every other path with edge. The two verifiers keep separate nonce
// caches. When internal and edge are the same verifier (no control-internal
// secret configured yet) this is the single shared-secret behaviour as before.
func SplitHMAC(internal, edge *HMACVerifier) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		viaInternal := internal.Middleware(next)
		viaEdge := edge.Middleware(next)
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if orchestratorOnlyRequest(r) {
				viaInternal.ServeHTTP(w, r)
				return
			}
			viaEdge.ServeHTTP(w, r)
		})
	}
}

// ControlInternalSecretPosture decides the secret for the orchestrator-only
// group from the two environment values. A missing control-internal secret falls
// back to the edge secret, so a deploy that has not set it yet keeps working;
// the second return value is the one warning the caller logs: "unset" when it
// fell back, "same" when it was set but equal to the edge secret (no isolation),
// "" when the two differ.
func ControlInternalSecretPosture(controlInternal, edge string) (secret, warning string) {
	switch {
	case controlInternal == "":
		return edge, "unset"
	case controlInternal == edge:
		return controlInternal, "same"
	default:
		return controlInternal, ""
	}
}
