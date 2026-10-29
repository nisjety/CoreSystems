// Package readiness performs a fixed, read-only verification of a connected
// marketing provider. It deliberately returns normalized status only: provider
// payloads and bearer tokens never leave this package.
package readiness

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/store"
)

var (
	// ErrUnsupportedProvider means the connection is not one this narrow
	// marketing readiness check understands.
	ErrUnsupportedProvider = errors.New("marketing readiness is not implemented for this provider")
	// ErrUnavailable means the host did not configure the token broker or
	// governed action executor required for a provider request.
	ErrUnavailable = errors.New("marketing readiness dependencies are not configured")
	// ErrProbeFailed wraps an upstream/provider failure. Callers must return a
	// normalized message rather than forwarding the wrapped provider response.
	ErrProbeFailed = errors.New("marketing readiness probe failed")
)

type State string

const (
	StateReady    State = "ready"
	StateNotReady State = "not_ready"
)

// Result contains only connection readiness signals. Counts are intentionally
// aggregate; site URLs, account identifiers, names, scope strings, tokens, and
// upstream provider response bodies are not represented here.
type Result struct {
	ProviderKey          string    `json:"providerKey"`
	State                State     `json:"state"`
	Operation            string    `json:"operation,omitempty"`
	CheckedAt            time.Time `json:"checkedAt"`
	SiteCount            int       `json:"siteCount"`
	AccountCount         int       `json:"accountCount"`
	InsightsEligible     bool      `json:"insightsEligible"`
	MissingPrerequisites []string  `json:"missingPrerequisites,omitempty"`
	NextAction           string    `json:"nextAction"`
}

// TokenLoader leases a token solely for the fixed provider request. The token
// is never stored in Result or passed to an untrusted caller.
type TokenLoader func(context.Context, string) (string, error)

// Executor is satisfied by the governed integration action service.
type Executor interface {
	Execute(context.Context, actions.ExecuteInput) (actions.ExecuteResult, error)
}

type Service struct {
	loadToken TokenLoader
	executor  Executor
	now       func() time.Time
}

func NewService(loadToken TokenLoader, executor Executor, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{loadToken: loadToken, executor: executor, now: now}
}

// Probe performs at most one fixed, read-only provider action. It does not
// accept an operation, resource ID, or provider parameters from the caller.
func (s *Service) Probe(ctx context.Context, connection store.Connection) (Result, error) {
	providerKey := strings.TrimSpace(connection.ProviderKey)
	result := Result{
		ProviderKey: providerKey,
		CheckedAt:   s.now().UTC(),
		NextAction:  "none",
	}
	if connection.DeletedAt != nil || connection.Status != "active" {
		return notReady(result, "reconnect", "active_connection"), nil
	}

	switch providerKey {
	case "google":
		return s.probeGoogle(ctx, connection, result)
	case "meta", "facebook", "meta-ads":
		return s.probeMeta(ctx, connection, result)
	default:
		return Result{}, ErrUnsupportedProvider
	}
}

func (s *Service) probeGoogle(ctx context.Context, connection store.Connection, result Result) (Result, error) {
	if !hasCapability(connection.Capabilities, "search_console.read") {
		return notReady(result, "grant_capability", "search_console.read"), nil
	}
	result.Operation = "gsc.sites.list"
	providerResult, err := s.execute(ctx, connection, result.Operation, nil)
	if err != nil {
		return Result{}, err
	}
	result.SiteCount = listCount(providerResult.Result, "siteEntry")
	if result.SiteCount == 0 {
		return notReady(result, "connect_property", "verified_search_console_property"), nil
	}
	result.State = StateReady
	return result, nil
}

func (s *Service) probeMeta(ctx context.Context, connection store.Connection, result Result) (Result, error) {
	if !hasCapability(connection.Capabilities, "social.ads.manage") {
		return notReady(result, "grant_capability", "social.ads.manage"), nil
	}
	result.Operation = "ads.adaccounts"
	providerResult, err := s.execute(ctx, connection, result.Operation, map[string]any{"limit": 1})
	if err != nil {
		return Result{}, err
	}
	result.AccountCount = listCount(providerResult.Result, "data")
	if result.AccountCount == 0 {
		return notReady(result, "connect_ad_account", "meta_ad_account"), nil
	}
	if !hasCapability(connection.Capabilities, "social.analytics.read") {
		return notReady(result, "grant_capability", "social.analytics.read"), nil
	}
	result.State = StateReady
	result.InsightsEligible = true
	return result, nil
}

func (s *Service) execute(ctx context.Context, connection store.Connection, operation string, params map[string]any) (actions.ExecuteResult, error) {
	if s.loadToken == nil || s.executor == nil {
		return actions.ExecuteResult{}, ErrUnavailable
	}
	token, err := s.loadToken(ctx, connection.ID)
	if err != nil {
		return actions.ExecuteResult{}, fmt.Errorf("%w: token lease: %v", ErrProbeFailed, err)
	}
	result, err := s.executor.Execute(ctx, actions.ExecuteInput{
		Connection:  connection,
		AccessToken: token,
		Operation:   operation,
		Params:      copyParams(params),
	})
	if err != nil {
		return actions.ExecuteResult{}, fmt.Errorf("%w: provider request: %v", ErrProbeFailed, err)
	}
	return result, nil
}

func notReady(result Result, nextAction, prerequisite string) Result {
	result.State = StateNotReady
	result.NextAction = nextAction
	result.MissingPrerequisites = []string{prerequisite}
	return result
}

func hasCapability(capabilities []string, required string) bool {
	for _, capability := range capabilities {
		if strings.TrimSpace(capability) == required {
			return true
		}
	}
	return false
}

func listCount(providerResult any, key string) int {
	object, ok := providerResult.(map[string]any)
	if !ok {
		return 0
	}
	values, ok := object[key].([]any)
	if !ok {
		return 0
	}
	return len(values)
}

func copyParams(input map[string]any) map[string]any {
	if input == nil {
		return nil
	}
	output := make(map[string]any, len(input))
	for key, value := range input {
		output[key] = value
	}
	return output
}
