package snapshot

import (
	"regexp"
	"strings"
)

const redacted = "[REDACTED]"

// scratchOnlyPrefixes are path prefixes whose contents never belong in a
// durable snapshot — the credential-free SCRATCH allowlist's own working
// directories (execution-core's sandbox.rs). A file under one of these is
// dropped entirely, not redacted: there is no reason to keep even a
// redacted trace of scratch-only content.
var scratchOnlyPrefixes = []string{"scratch/", "/tmp/"}

// stringPatterns and capturePatterns port execution-core's scrub.rs
// (string_patterns/capture_patterns, scrub.rs:47-107) independently for Go:
// same pattern set — Bearer/Basic auth headers, JWTs, sk-* tokens, AWS
// access key ids, GitHub/Slack tokens, Google API keys, PEM private key
// blocks, connection-string userinfo, and inline key=value assignments.
// Capture patterns keep surrounding context (the key name, the URI
// scheme/host) and redact only the secret portion, same as the Rust source.
var stringPatterns = []*regexp.Regexp{
	regexp.MustCompile(`(?i)bearer\s+[A-Za-z0-9._\-+/=]+`),
	regexp.MustCompile(`eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+`),
	regexp.MustCompile(`sk-[A-Za-z0-9_\-]{16,}`),
	regexp.MustCompile(`AKIA[0-9A-Z]{16}`),
	regexp.MustCompile(`gh[pousr]_[A-Za-z0-9]{20,}`),
	regexp.MustCompile(`xox[baprs]-[A-Za-z0-9-]{10,}`),
	regexp.MustCompile(`AIza[0-9A-Za-z_\-]{35}`),
	regexp.MustCompile(`(?i)basic\s+[A-Za-z0-9+/=]{8,}`),
	regexp.MustCompile(`-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----`),
}

type capturePattern struct {
	re          *regexp.Regexp
	replacement string
}

var capturePatterns = []capturePattern{
	// scheme://user:SECRET@host -> scheme://user:[REDACTED]@host
	{regexp.MustCompile(`(?i)\b([a-z][a-z0-9+.\-]*://[^:/?#\s]+:)([^@/?#\s]+)(@)`), "${1}" + redacted + "${3}"},
	// DB_PASSWORD=secret | api_key: "xyz" -> DB_PASSWORD=[REDACTED]
	{regexp.MustCompile(`(?i)\b([a-z0-9_]*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|auth[_-]?token|client[_-]?secret|private[_-]?key|token))(\s*[:=]\s*)("?)([^\s"'&,;]{3,})`), "${1}${2}${3}" + redacted},
}

// scrubString redacts known secret patterns inside file content. See
// stringPatterns/capturePatterns above for the exact pattern set ported
// from execution-core's scrub.rs.
func scrubString(s string) string {
	out := s
	for _, re := range stringPatterns {
		out = re.ReplaceAllString(out, redacted)
	}
	for _, pattern := range capturePatterns {
		out = pattern.re.ReplaceAllString(out, pattern.replacement)
	}
	return out
}

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
		out[key] = []byte(scrubString(string(content)))
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
