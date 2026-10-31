package readiness

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/store"
)

type fakeExecutor struct {
	inputs []actions.ExecuteInput
	result actions.ExecuteResult
	err    error
}

func (f *fakeExecutor) Execute(_ context.Context, input actions.ExecuteInput) (actions.ExecuteResult, error) {
	f.inputs = append(f.inputs, input)
	return f.result, f.err
}

func TestProbeGoogleSearchConsoleUsesFixedReadOnlyActionAndRedactsProviderPayload(t *testing.T) {
	executor := &fakeExecutor{result: actions.ExecuteResult{Result: map[string]any{
		"siteEntry": []any{map[string]any{"siteUrl": "sc-domain:private.example", "permissionLevel": "siteOwner"}},
	}}}
	tokenCalls := 0
	checkedAt := time.Date(2026, time.October, 19, 10, 0, 0, 0, time.UTC)
	service := NewService(func(context.Context, string) (string, error) {
		tokenCalls++
		return "google-access-token", nil
	}, executor, func() time.Time { return checkedAt })

	result, err := service.Probe(t.Context(), store.Connection{
		ID: "google-connection", ProviderKey: "google", Status: "active", Capabilities: []string{"search_console.read"},
	})
	if err != nil {
		t.Fatalf("Probe error: %v", err)
	}
	if result.State != StateReady || result.Operation != "gsc.sites.list" || result.SiteCount != 1 || !result.CheckedAt.Equal(checkedAt) {
		t.Fatalf("unexpected Google readiness result: %#v", result)
	}
	if tokenCalls != 1 || len(executor.inputs) != 1 {
		t.Fatalf("token/executor calls = %d/%d, want 1/1", tokenCalls, len(executor.inputs))
	}
	input := executor.inputs[0]
	if input.Operation != "gsc.sites.list" || input.AccessToken != "google-access-token" || input.Params != nil || input.Body != nil {
		t.Fatalf("Google probe input = %#v, want fixed empty GET action", input)
	}

	payload, err := json.Marshal(result)
	if err != nil {
		t.Fatalf("marshal result: %v", err)
	}
	for _, forbidden := range []string{"google-access-token", "sc-domain:private.example", "siteOwner", "siteEntry"} {
		if strings.Contains(string(payload), forbidden) {
			t.Fatalf("readiness response leaked %q: %s", forbidden, payload)
		}
	}
}

func TestProbeMetaUsesOneBoundedReadOnlyActionAndReportsInsightEligibility(t *testing.T) {
	executor := &fakeExecutor{result: actions.ExecuteResult{Result: map[string]any{
		"data": []any{map[string]any{"id": "act_123", "name": "Private account", "currency": "NOK"}},
	}}}
	service := NewService(func(context.Context, string) (string, error) {
		return "meta-access-token", nil
	}, executor, nil)

	result, err := service.Probe(t.Context(), store.Connection{
		ID: "meta-connection", ProviderKey: "meta", Status: "active", Capabilities: []string{"social.ads.manage", "social.analytics.read"},
	})
	if err != nil {
		t.Fatalf("Probe error: %v", err)
	}
	if result.State != StateReady || result.Operation != "ads.adaccounts" || result.AccountCount != 1 || !result.InsightsEligible {
		t.Fatalf("unexpected Meta readiness result: %#v", result)
	}
	if len(executor.inputs) != 1 {
		t.Fatalf("executor inputs = %d, want 1", len(executor.inputs))
	}
	input := executor.inputs[0]
	if input.Operation != "ads.adaccounts" || !reflect.DeepEqual(input.Params, map[string]any{"limit": 1}) || input.Body != nil {
		t.Fatalf("Meta probe input = %#v, want fixed bounded GET action", input)
	}
	payload, err := json.Marshal(result)
	if err != nil {
		t.Fatalf("marshal result: %v", err)
	}
	for _, forbidden := range []string{"meta-access-token", "act_123", "Private account", "currency"} {
		if strings.Contains(string(payload), forbidden) {
			t.Fatalf("readiness response leaked %q: %s", forbidden, payload)
		}
	}
}

func TestProbeMissingCapabilityDoesNotLoadTokenOrContactProvider(t *testing.T) {
	tokenCalls := 0
	executor := &fakeExecutor{}
	service := NewService(func(context.Context, string) (string, error) {
		tokenCalls++
		return "should-not-be-loaded", nil
	}, executor, nil)

	result, err := service.Probe(t.Context(), store.Connection{
		ID: "google-without-gsc", ProviderKey: "google", Status: "active", Capabilities: []string{"profile.read"},
	})
	if err != nil {
		t.Fatalf("Probe error: %v", err)
	}
	if result.State != StateNotReady || !reflect.DeepEqual(result.MissingPrerequisites, []string{"search_console.read"}) {
		t.Fatalf("result = %#v, want missing Search Console capability", result)
	}
	if tokenCalls != 0 || len(executor.inputs) != 0 {
		t.Fatalf("token/executor calls = %d/%d, want 0/0", tokenCalls, len(executor.inputs))
	}
}

func TestProbeInactiveConnectionDoesNotLoadTokenOrContactProvider(t *testing.T) {
	tokenCalls := 0
	executor := &fakeExecutor{}
	service := NewService(func(context.Context, string) (string, error) {
		tokenCalls++
		return "should-not-be-loaded", nil
	}, executor, nil)

	result, err := service.Probe(t.Context(), store.Connection{
		ID: "inactive-meta", ProviderKey: "meta", Status: "needs_refresh", Capabilities: []string{"social.ads.manage"},
	})
	if err != nil {
		t.Fatalf("Probe error: %v", err)
	}
	if result.State != StateNotReady || !reflect.DeepEqual(result.MissingPrerequisites, []string{"active_connection"}) || result.NextAction != "reconnect" {
		t.Fatalf("result = %#v, want reconnect state", result)
	}
	if tokenCalls != 0 || len(executor.inputs) != 0 {
		t.Fatalf("token/executor calls = %d/%d, want 0/0", tokenCalls, len(executor.inputs))
	}
}
