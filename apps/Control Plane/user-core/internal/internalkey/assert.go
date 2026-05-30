// Package internalkey enforces a boot-time format check on the cluster-wide
// internal API key.
//
// G40 (velion-gap.md §8.29): three production incidents — G30, §8.23, §8.27 —
// were caused by a service starting with a placeholder / drifted internal
// API key, and the mismatch only surfacing on the first request that needed
// it. Velion closed its side in §8.27; G40 mirrors the gate in CP Go
// services so a misconfigured CP container also refuses to start (in release
// mode) rather than serve a green `/health` and 401 every cross-service
// call.
//
// This file is a canonical copy duplicated across the four CP Go services
// because they live in separate Go modules with different module roots
// (`AquatiqCMS` vs `CoreSystem`). Adding a shared workspace just for one
// helper would cost more friction than the duplication. If you change
// this file, sync the other three copies under
// `apps/Control Plane/{billing-core,user-core,org-core,session-core}/
//  internal/internalkey/assert.go`.
package internalkey

import (
	"fmt"
	"os"
	"strings"
)

// PlaceholderPrefixes lists the values the helper recognises as
// placeholders. Matches velion's `scripts/check-internal-api-keys.mjs` and
// `src/lib/server/internal-api-key-assertion.ts` so the cross-language
// behaviour is consistent.
var placeholderPrefixes = []string{
	"test",
	"placeholder",
	"change-me",
	"your-",
	"replace-me",
}

const minKeyLength = 32

// ProblemKind describes why a value failed validation.
type ProblemKind string

const (
	ProblemMissing     ProblemKind = "missing"
	ProblemPlaceholder ProblemKind = "placeholder"
	ProblemTooShort    ProblemKind = "too_short"
)

// Problem is the structured failure reason. `EnvVar` is the env-var name
// the caller picked from (after fallback resolution).
type Problem struct {
	Kind   ProblemKind
	EnvVar string
	Detail string
}

// Resolve returns the first non-empty value among the given env-var names
// (the first one wins on ties). Returns the matched name + value, or ("",
// "") when none are set.
func Resolve(envVars ...string) (string, string) {
	for _, name := range envVars {
		raw := strings.TrimSpace(os.Getenv(name))
		if raw != "" {
			return name, raw
		}
	}
	return "", ""
}

// Validate runs the three format checks against `value`. `envVarName` is
// the resolved env var the value came from (used in the Problem detail).
// Returns nil when the value passes.
func Validate(envVarName, value string) *Problem {
	if value == "" {
		return &Problem{
			Kind:   ProblemMissing,
			EnvVar: envVarName,
			Detail: "value is empty — configure the cluster-wide secret",
		}
	}
	lowered := strings.ToLower(value)
	for _, prefix := range placeholderPrefixes {
		// Plain prefix-match. The listed prefixes (`test`, `placeholder`,
		// `change-me`, `your-`, `replace-me`) are not valid hex characters
		// past position 0 so a real 64-char hex secret cannot collide with
		// them. Sites that already include their own anchor (e.g. `your-`)
		// match exactly; bare words (e.g. `test`) match common forms like
		// `test`, `test-key`, `test_secret` without extra logic.
		if strings.HasPrefix(lowered, prefix) {
			return &Problem{
				Kind:   ProblemPlaceholder,
				EnvVar: envVarName,
				Detail: fmt.Sprintf("value looks like a placeholder (%q…) — set the real cluster-wide secret", truncate(value, 16)),
			}
		}
	}
	if len(value) < minKeyLength {
		return &Problem{
			Kind:   ProblemTooShort,
			EnvVar: envVarName,
			Detail: fmt.Sprintf("value is only %d chars; canonical secret is 64-char hex (min %d)", len(value), minKeyLength),
		}
	}
	return nil
}

// IsProduction returns true when the process is running in a production-
// like mode and a failed format check should be fatal. We treat both
// `GIN_MODE=release` (gin-based services run this way in Docker) and
// `ENV=production` / `NODE_ENV=production` as production-like.
func IsProduction() bool {
	if strings.EqualFold(os.Getenv("GIN_MODE"), "release") {
		return true
	}
	for _, name := range []string{"ENV", "NODE_ENV", "GO_ENV", "APP_ENV"} {
		if strings.EqualFold(os.Getenv(name), "production") {
			return true
		}
	}
	return false
}

// AssertResult is what AssertFromEnv returns. `OK` is the simple top-level
// signal; callers wanting nicely-formatted failure detail can render the
// `Problem` field themselves.
type AssertResult struct {
	OK       bool
	EnvVar   string
	Problem  *Problem
	Resolved string // the matched env-var name (empty on Missing)
}

// AssertFromEnv resolves the value from the first non-empty of `envVars`,
// validates it, and returns the structured result. It does NOT call
// `log.Fatal` itself — the caller decides how to surface the result so
// services can keep their own logger style (zerolog, gin's logger, etc.).
//
// Typical caller (from each CP Go service's `main.go`):
//
//	if r := internalkey.AssertFromEnv("INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"); !r.OK {
//	    if internalkey.IsProduction() {
//	        log.Fatal().Str("env_var", r.Problem.EnvVar).Str("kind", string(r.Problem.Kind)).
//	            Msg("[service startup] FATAL: " + r.Problem.Detail)
//	    }
//	    log.Warn().Str("env_var", r.Problem.EnvVar).Str("kind", string(r.Problem.Kind)).
//	        Msg("[service startup] WARN: " + r.Problem.Detail + " — continuing because not production")
//	}
func AssertFromEnv(envVars ...string) AssertResult {
	name, value := Resolve(envVars...)
	if value == "" {
		// Report against the first env var so log lines are predictable.
		first := ""
		if len(envVars) > 0 {
			first = envVars[0]
		}
		return AssertResult{
			OK: false,
			Problem: &Problem{
				Kind:   ProblemMissing,
				EnvVar: first,
				Detail: fmt.Sprintf("none of %v are set — configure the cluster-wide secret", envVars),
			},
		}
	}
	if problem := Validate(name, value); problem != nil {
		return AssertResult{OK: false, EnvVar: name, Problem: problem, Resolved: name}
	}
	return AssertResult{OK: true, EnvVar: name, Resolved: name}
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
