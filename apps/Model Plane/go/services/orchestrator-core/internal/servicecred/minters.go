package servicecred

import (
	"fmt"
	"log/slog"
	"strings"

	"github.com/triodelab/model-plane/pkg/servicetoken"
)

// Options describes this deployment's minting identity.
type Options struct {
	// AuthCoreURL is Auth Core's base URL. Empty disables minting entirely.
	AuthCoreURL string
	// ServiceID is orchestrator-core's service-principal id (`x-service-id`).
	ServiceID string
	// Credential is that principal's secret (`x-service-api-key`). Never logged.
	Credential string
	// ScopesByAudience overrides the per-audience scope request. An audience
	// absent from the map uses [DefaultScopes]; an audience mapped to an empty
	// list is disabled, which is how an operator turns one audience off without
	// removing the credential.
	ScopesByAudience map[string][]string
}

// DefaultScopes are the scopes each audience needs for the RPCs orchestrator-core
// actually makes. They are minimal on purpose: the mint request can only narrow
// the principal's registry ceiling, so asking for less than the ceiling is free,
// while asking for more is refused outright (403) instead of quietly narrowed.
//
//   - session-core `session:write` — StartRunActivity's StartRun. It must be a
//     write scope AND the credential must not be ZDR: session-core's
//     authorize_operation refuses any ZDR credential for a non-`:read` scope, so
//     the principal's retentionByAudience for session-core has to be
//     `persistent` or every durable run fails its very first activity.
//   - inference-core `inference:invoke` — the evaluator-optimizer's generator
//     and judge legs.
//   - capability-core `capability:read` + `capability:write` — the skill
//     promotion gate reads, PromoteSkill writes.
//   - letta-bridge `memory:read` + `memory:write` — memory consolidation reads
//     entries and writes the consolidated summaries back.
var DefaultScopes = map[string][]string{
	AudienceSessionCore:    {"session:write"},
	AudienceInferenceCore:  {"inference:invoke"},
	AudienceCapabilityCore: {"capability:read", "capability:write"},
	AudienceLettaBridge:    {"memory:read", "memory:write"},
}

// mintReason is audited by Auth Core. It names the concrete caller so an
// operator reading the audit trail can tell orchestrator-core's activity traffic
// apart from its proxy traffic.
const mintReason = "orchestrator-core durable workflow activity"

// Minters holds one minter per audience. A nil entry means "not configured for
// this audience", which the interceptor degrades to plain forwarding.
type Minters map[string]Minter

// Get returns the minter for audience, or nil when none is configured.
// A nil Minters is valid and returns nil for every audience.
func (m Minters) Get(audience string) Minter {
	if m == nil {
		return nil
	}
	return m[audience]
}

// NewMinters builds the per-audience minters from opts.
//
// A partial failure is not fatal: each audience is independent, so a bad scope
// list for one must not take down the others. Failures are logged at WARN with
// the audience named, and that audience degrades to no credential — the same
// state it was in before this package existed. The returned error is non-nil
// only when the whole thing is unconfigured, which callers report as a single
// startup warning.
func NewMinters(opts Options, logger *slog.Logger) (Minters, error) {
	if logger == nil {
		logger = slog.Default()
	}
	if strings.TrimSpace(opts.AuthCoreURL) == "" {
		return nil, fmt.Errorf("servicecred: AUTH_CORE_URL is not set, activities will call siblings with no credential")
	}
	if strings.TrimSpace(opts.ServiceID) == "" || strings.TrimSpace(opts.Credential) == "" {
		return nil, fmt.Errorf(
			"servicecred: ORCHESTRATOR_CORE_SERVICE_ID and " +
				"ORCHESTRATOR_CORE_SERVICE_API_KEY are both required to mint tokens")
	}

	out := Minters{}
	for _, audience := range []string{
		AudienceSessionCore,
		AudienceInferenceCore,
		AudienceCapabilityCore,
		AudienceLettaBridge,
	} {
		scopes := DefaultScopes[audience]
		if override, ok := opts.ScopesByAudience[audience]; ok {
			scopes = override
		}
		if len(scopes) == 0 {
			logger.Info("service token minting disabled for audience", "audience", audience)
			continue
		}
		provider, err := servicetoken.New(servicetoken.Config{
			AuthCoreURL: opts.AuthCoreURL,
			ServiceID:   opts.ServiceID,
			Credential:  opts.Credential,
			Audience:    audience,
			Scopes:      scopes,
			Reason:      mintReason,
		})
		if err != nil {
			logger.Warn("service token minting unavailable for audience",
				"audience", audience, "err", err)
			continue
		}
		out[audience] = provider
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("servicecred: no audience could be configured")
	}
	return out, nil
}
