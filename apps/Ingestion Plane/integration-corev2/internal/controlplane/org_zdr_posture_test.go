package controlplane

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

// The org-core posture route is the one the ZDR acknowledgement uses to learn whether the
// organisation's ZDR is in force. The fixture mirrors org-core's real response body
// ({zdrIntent, entitled}, org-core internal/org RetentionPosture) and the real route path.
func TestOrgRetentionPostureIsEffectiveOnlyWhenIntentAndEntitlementBothHold(t *testing.T) {
	cases := []struct {
		name string
		body map[string]any
		want bool
	}{
		{"intent and entitlement", map[string]any{"zdrIntent": true, "entitled": true}, true},
		{"a lapsed plan keeps its intent but is not effective", map[string]any{"zdrIntent": true, "entitled": false}, false},
		{"no intent", map[string]any{"zdrIntent": false, "entitled": true}, false},
		{"neither", map[string]any{"zdrIntent": false, "entitled": false}, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				_ = json.NewEncoder(w).Encode(tc.body)
			}))
			defer server.Close()
			cfg := testControlPlaneConfig("http://unused")
			cfg.OrgCoreURL = server.URL
			got, err := NewOrgClient(cfg, server.Client()).EffectiveZDR(t.Context(), "org-1")
			if err != nil || got != tc.want {
				t.Fatalf("EffectiveZDR = (%v, %v), want (%v, nil)", got, err, tc.want)
			}
		})
	}
}

func TestOrgRetentionPostureReadsTheInternalRouteWithTheServicePrincipal(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/internal/orgs/org-1/retention-posture" {
			t.Fatalf("request = %s %s, want GET /internal/orgs/org-1/retention-posture", r.Method, r.URL.Path)
		}
		if got := r.Header.Get("X-Service-Id"); got != "integration-corev2" {
			t.Fatalf("service id = %q, want integration-corev2", got)
		}
		if got := r.Header.Get("X-Service-Token"); got != "org-service-token-at-least-32-bytes" {
			t.Fatalf("service token = %q", got)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "" {
			t.Fatalf("legacy key leaked = %q", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"zdrIntent": true, "entitled": true})
	}))
	defer server.Close()
	cfg := testControlPlaneConfig("http://unused")
	cfg.OrgCoreURL = server.URL
	if got, err := NewOrgClient(cfg, server.Client()).EffectiveZDR(t.Context(), "org-1"); err != nil || !got {
		t.Fatalf("EffectiveZDR = (%v, %v)", got, err)
	}
}

// An unknown posture is never read as "off" and never as "on": every failure is an error, so the
// caller can refuse the acknowledgement (fail closed) rather than record a guess.
func TestOrgRetentionPostureFailsClosedOnAnyFailure(t *testing.T) {
	cases := []struct {
		name    string
		status  int
		body    string
		baseURL string
	}{
		{"server error", http.StatusInternalServerError, `{"error":"x"}`, ""},
		{"forbidden without the scope", http.StatusForbidden, `{"error":"forbidden"}`, ""},
		{"not found", http.StatusNotFound, `{"error":"organization not found"}`, ""},
		{"non-JSON body", http.StatusOK, `<html>`, ""},
		{"unconfigured", 0, "", "unconfigured"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(tc.status)
				_, _ = w.Write([]byte(tc.body))
			}))
			defer server.Close()
			cfg := testControlPlaneConfig("http://unused")
			cfg.OrgCoreURL = server.URL
			if tc.baseURL == "unconfigured" {
				cfg.OrgCoreURL = ""
			}
			got, err := NewOrgClient(cfg, server.Client()).EffectiveZDR(t.Context(), "org-1")
			if err == nil || got {
				t.Fatalf("EffectiveZDR = (%v, %v), want an error and not on", got, err)
			}
		})
	}
}
