// Package redact removes known secret shapes from text before it is
// persisted durably.
//
// The pattern set is an independent Go port of execution-core's scrub.rs
// (string_patterns/capture_patterns, scrub.rs:47-107): Bearer/Basic auth
// headers, JWTs, sk-* tokens, AWS access key ids, GitHub/Slack tokens,
// Google API keys, PEM private key blocks, connection-string userinfo, and
// inline key=value assignments. Capture patterns keep the surrounding
// context (the key name, the URI scheme/host) and redact only the secret
// portion, same as the Rust source — a scrubbed line has to stay
// diagnosable.
//
// It lived unexported in internal/snapshot until S4.2 gave it a second
// consumer (the process registry's command identity). Moving it here rather
// than duplicating it keeps one pattern set for the whole service; the
// existing snapshot tests are the regression lock on that move.
package redact

import (
	"regexp"
	"strings"
)

// Redacted replaces every secret this package recognizes.
const Redacted = "[REDACTED]"

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
	{regexp.MustCompile(`(?i)\b([a-z][a-z0-9+.\-]*://[^:/?#\s]+:)([^@/?#\s]+)(@)`), "${1}" + Redacted + "${3}"},
	// DB_PASSWORD=secret | api_key: "xyz" | --token=abc -> key preserved, value redacted.
	// This also covers the =-joined flag form, since `-` is a non-word
	// character so \b holds between `--` and the key name.
	{regexp.MustCompile(`(?i)\b([a-z0-9_]*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|auth[_-]?token|client[_-]?secret|private[_-]?key|token))(\s*[:=]\s*)("?)([^\s"'&,;]{3,})`), "${1}${2}${3}" + Redacted},
}

// secretFlags are argv flag names whose NEXT element is the secret.
//
// This is the one rule neither scrub.rs nor the original snapshot port has:
// every pattern above keys on a `:` or `=` separator, so a space-separated
// pair (`--token abc123`, `-p hunter2`) passes through all of them
// untouched. Matching is positional and cannot be done on a joined string,
// which is why Command exists at all rather than callers doing
// String(strings.Join(argv, " ")).
//
// Deliberately narrow. `--key` and `-k` are NOT here: `--key` is usually a
// file path (ssh, openssl) and `-k` is curl's insecure flag, so including
// them would redact ordinary arguments and teach readers to distrust the
// redaction. Over-redacting a non-secret costs diagnosability; under-
// redacting costs a leak — but only for names that are genuinely ambiguous
// is that trade worth taking, and these are not.
var secretFlags = map[string]struct{}{
	"token":         {},
	"auth-token":    {},
	"access-token":  {},
	"refresh-token": {},
	"password":      {},
	"passwd":        {},
	"pwd":           {},
	"pass":          {},
	"p":             {},
	"secret":        {},
	"client-secret": {},
	"secret-key":    {},
	"api-key":       {},
	"apikey":        {},
	"access-key":    {},
	"private-key":   {},
	"credential":    {},
	"credentials":   {},
	"auth":          {},
}

// String redacts known secret patterns inside free-form text. It is the same
// function snapshot payload content and process output both run through.
func String(s string) string {
	out := s
	for _, re := range stringPatterns {
		out = re.ReplaceAllString(out, Redacted)
	}
	for _, pattern := range capturePatterns {
		out = pattern.re.ReplaceAllString(out, pattern.replacement)
	}
	return out
}

// Command redacts a command's identity for durable storage: String over the
// program and every argument, plus the positional secretFlags rule that no
// text pattern can express.
//
// It returns copies; the caller's slice is never mutated, because the
// unredacted argv is still needed to actually spawn the process.
func Command(program string, args []string) (string, []string) {
	redactedArgs := make([]string, len(args))
	for i, arg := range args {
		redactedArgs[i] = String(arg)
	}
	for i, arg := range args {
		if i+1 >= len(args) {
			break
		}
		if !isSecretFlag(arg) {
			continue
		}
		// A following token that is itself a flag is the next option, not
		// this one's value (`--token --verbose` is a missing value, not a
		// secret named "--verbose").
		if strings.HasPrefix(args[i+1], "-") {
			continue
		}
		redactedArgs[i+1] = Redacted
	}
	return String(program), redactedArgs
}

// isSecretFlag reports whether arg is a flag whose value must be redacted.
// Normalizes `--API_KEY` / `-api-key` / `--api-key` to one spelling; an
// `=`-joined form is left to the capture pattern in String.
func isSecretFlag(arg string) bool {
	if !strings.HasPrefix(arg, "-") {
		return false
	}
	name := strings.TrimLeft(arg, "-")
	if name == "" || strings.Contains(name, "=") {
		return false
	}
	name = strings.ReplaceAll(strings.ToLower(name), "_", "-")
	_, ok := secretFlags[name]
	return ok
}
