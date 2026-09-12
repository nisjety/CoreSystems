package snapshot

import (
	"strings"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/redact"
)

// scratchOnlyPrefixes are path prefixes whose contents never belong in a
// durable snapshot — the credential-free SCRATCH allowlist's own working
// directories (execution-core's sandbox.rs). A file under one of these is
// dropped entirely, not redacted: there is no reason to keep even a
// redacted trace of scratch-only content.
var scratchOnlyPrefixes = []string{"scratch/", "/tmp/"}

// ExcludeCredentials prepares a snapshot payload for durable storage: any
// file under a scratch-only path is dropped outright — never written, not
// even redacted — and every remaining file's content is redacted in place.
// Call this immediately before the storage write, never after.
func ExcludeCredentials(files map[string][]byte) map[string][]byte {
	out := make(map[string][]byte, len(files))
	for key, content := range files {
		if isScratchOnly(key) {
			continue
		}
		out[key] = []byte(redact.String(string(content)))
	}
	return out
}

func isScratchOnly(key string) bool {
	for _, prefix := range scratchOnlyPrefixes {
		if strings.HasPrefix(key, prefix) {
			return true
		}
	}
	return false
}
