package http

import (
	"net/http"
	"strings"
)

// Space scope table seam (plan P-UC1). The cases below moved unchanged from
// serviceScopeForRequest in service_auth.go. A feature adds its scopes from
// its own scopes_<feature>.go init() through registerSpaceScopeRule and
// registerSpaceDelegatedScopePrefix, so no feature edits this table or
// service_auth.go.

type spaceScopeRule func(method, path string) (string, bool)

var (
	spaceFeatureScopeRules       []spaceScopeRule
	spaceDelegatedScopePrefixes []string
)

// registerSpaceScopeRule adds a feature path-to-scope rule. Feature rules run
// only after the core table below has not matched, so a feature can never
// re-scope or widen an existing route.
func registerSpaceScopeRule(rule spaceScopeRule) {
	spaceFeatureScopeRules = append(spaceFeatureScopeRules, rule)
}

// registerSpaceDelegatedScopePrefix marks every scope with this prefix as one
// that requires a verified delegation token (a service credential alone never
// proves an end user).
func registerSpaceDelegatedScopePrefix(prefix string) {
	spaceDelegatedScopePrefixes = append(spaceDelegatedScopePrefixes, prefix)
}

func spaceScopeRequiresVerifiedDelegation(scope string) bool {
	if scope == "spaces:resolve" || scope == "spaces:issue" {
		return true
	}
	for _, prefix := range spaceDelegatedScopePrefixes {
		if strings.HasPrefix(scope, prefix) {
			return true
		}
	}
	return false
}

// serviceScopeForSpacePath returns the scope for an internal spaces path, or
// ("", false) when the path is not a spaces path this table knows.
func serviceScopeForSpacePath(method, path string) (string, bool) {
	if scope, ok := coreSpaceScope(method, path); ok {
		return scope, true
	}
	for _, rule := range spaceFeatureScopeRules {
		if scope, ok := rule(method, path); ok {
			return scope, true
		}
	}
	return "", false
}

func coreSpaceScope(method, path string) (string, bool) {
	switch {
	case path == "/api/v1/internal/spaces/register":
		return "spaces:register", true
	case path == "/api/v1/internal/spaces/deletion-authorizations":
		return "spaces:deletion:authorize", true
	case path == "/api/v1/internal/spaces/deletion-policy":
		return "spaces:policy:write", true
	case strings.HasPrefix(path, "/api/v1/internal/spaces/") && strings.HasSuffix(path, "/legal-hold"):
		return "spaces:policy:write", true
	case method == http.MethodGet && strings.HasPrefix(path, "/api/v1/internal/spaces/") && strings.HasSuffix(path, "/roster"):
		return "spaces:resolve", true
	case method == http.MethodGet && (path == "/api/v1/internal/spaces" || path == "/api/v1/internal/spaces/"):
		return "spaces:resolve", true
	case method == http.MethodPut && strings.HasPrefix(path, "/api/v1/internal/spaces/") && strings.HasSuffix(path, "/memberships"):
		return "spaces:membership:write", true
	case path == "/api/v1/internal/spaces/recipient-audiences":
		return "spaces:audience:publish", true
	case method == http.MethodGet && strings.HasPrefix(path, "/api/v1/internal/spaces/") && strings.HasSuffix(path, "/membership"):
		return "spaces:resolve", true
	case path == "/api/v1/internal/spaces/personal-thread-decision" || path == "/api/v1/internal/spaces/thread-decision" || path == "/api/v1/internal/spaces/thread-append-decision" || path == "/api/v1/internal/spaces/thread-read-decision" || path == "/api/v1/internal/spaces/retrieval-decision" || path == "/api/v1/internal/spaces/personal-retrieval-decision" || path == "/api/v1/internal/spaces/personal-import-decision" || path == "/api/v1/internal/spaces/schedule-create-decision" || path == "/api/v1/internal/spaces/watch-create-decision" || path == "/api/v1/internal/spaces/watch-observe-decision" || path == "/api/v1/internal/spaces/owner-grant-decision":
		return "spaces:issue", true
	case path == "/api/v1/internal/spaces/import-execution-decision":
		return "spaces:import:reauthorize", true
	case path == "/api/v1/internal/spaces/schedule-fire-decision" || path == "/api/v1/internal/spaces/scheduled-run-decision":
		return "spaces:schedule:reauthorize", true
	case path == "/api/v1/internal/spaces/sandbox-capability-decision":
		return "spaces:sandbox:capability", true
	case path == "/api/v1/internal/spaces/scheduled-run-execution-decision":
		return "spaces:schedule:execute", true
	case path == "/api/v1/internal/spaces/scheduled-step-decision":
		return "spaces:schedule:step", true
	case path == "/api/v1/internal/spaces/model-action-view":
		return "spaces:agent-action:view", true
	case path == "/api/v1/internal/spaces/run-action-decision":
		return "spaces:agent-action:reauthorize", true
	case path == "/api/v1/internal/spaces/run-action-authority-check":
		return "spaces:agent-action:current-authority", true
	case strings.HasPrefix(path, "/api/v1/internal/spaces/owner-effect-reservations"):
		return "spaces:agent-action:reservation", true
	case path == "/api/v1/internal/spaces/effect-policy":
		return "spaces:policy:write", true
	}
	return "", false
}

