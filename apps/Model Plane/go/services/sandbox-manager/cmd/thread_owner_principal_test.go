package main

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadowner"
)

// The checks below pin the principal half of the thread owner check (P2-go-07-rel1). The
// ownership read asks session-core as sandbox-manager's own service principal, naming one
// narrow scope. Three things must agree or every thread-scoped call is refused as UNAVAILABLE:
// the token request this file drives, the Control Plane registry row Auth Core mints it from,
// and the session-core gate that accepts it (session-core auth.rs,
// authorize_thread_owner_resolve).

// principalFixtureKey is a low-entropy fake service credential, for the mint test only.
const principalFixtureKey = "fixture-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa"

// principalEnvOf reads a fixed environment, as the switch's getenv does.
func principalEnvOf(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

// TestThreadOwnerTokenIsMintedForSessionCoreAsTheSandboxManagerPrincipal builds the token source
// the switch builds, against a fake Auth Core, and reads the mint request it sends: the
// sandbox-manager principal, its own credential, the session-core audience, and exactly the one
// thread owner scope, for the org it asks about.
func TestThreadOwnerTokenIsMintedForSessionCoreAsTheSandboxManagerPrincipal(t *testing.T) {
	var (
		gotPath       string
		gotServiceID  string
		gotCredential string
		gotBody       map[string]any
	)
	authCore := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotServiceID = r.Header.Get("x-service-id")
		gotCredential = r.Header.Get("x-service-api-key")
		raw, err := io.ReadAll(r.Body)
		if err != nil || json.Unmarshal(raw, &gotBody) != nil {
			http.Error(w, "unreadable mint request", http.StatusBadRequest)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"token":"fixture-token-sandbox-manager","expiresInSeconds":300,"audience":"session-core"}`)
	}))
	defer authCore.Close()

	wiring, err := threadOwnerFromEnv(principalEnvOf(map[string]string{
		"AUTH_CORE_URL":                   authCore.URL,
		"SANDBOX_MANAGER_SERVICE_API_KEY": principalFixtureKey,
	}))
	if err != nil {
		t.Fatalf("threadOwnerFromEnv: %v", err)
	}
	defer func() { _ = wiring.conn.Close() }()

	token, err := wiring.tokens.Token(context.Background(), "org-1")
	if err != nil {
		t.Fatalf("Token: %v", err)
	}
	if token != "fixture-token-sandbox-manager" {
		t.Fatalf("Token = %q, want the token Auth Core minted", token)
	}
	if gotPath != "/api/session-core/internal-token" {
		t.Fatalf("mint path = %q, want the session-core route", gotPath)
	}
	if gotServiceID != defaultServiceID {
		t.Fatalf("x-service-id = %q, want the sandbox-manager principal %q", gotServiceID, defaultServiceID)
	}
	if gotCredential != principalFixtureKey {
		t.Fatal("x-service-api-key is not the SANDBOX_MANAGER_SERVICE_API_KEY credential")
	}
	if gotBody["orgId"] != "org-1" {
		t.Fatalf("mint orgId = %v, want the org the thread belongs to", gotBody["orgId"])
	}
	if scopes, _ := gotBody["scopes"].([]any); len(scopes) != 1 || scopes[0] != threadowner.Scope {
		t.Fatalf("mint scopes = %v, want exactly %q", gotBody["scopes"], threadowner.Scope)
	}
	if _, present := gotBody["zdr"]; present {
		t.Fatal("the mint request names a zdr posture: retention is the deployment policy's to choose")
	}
}

// principalPolicyRow is the part of one Control Plane principal row this check reads. The policy
// file is the one Auth Core loads its registry from, so the row is read exactly as written.
type principalPolicyRow struct {
	Tenancy             string              `json:"tenancy"`
	Audiences           []string            `json:"audiences"`
	ConsumerEnv         string              `json:"consumerEnv"`
	Scopes              []string            `json:"scopes"`
	ScopesByAudience    map[string][]string `json:"scopesByAudience"`
	RetentionByAudience map[string]string   `json:"retentionByAudience"`
}

// TestRegistryGrantsSandboxManagerTheThreadOwnerScopeAndNoOther reads the checked-in principal
// policy. Auth Core mints the token only for a row keyed sandbox-manager, so that row must be a
// directory-bound principal that addresses session-core alone, persistently, with exactly the one
// scope. No other principal may hold the scope.
func TestRegistryGrantsSandboxManagerTheThreadOwnerScopeAndNoOther(t *testing.T) {
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	policyPath := filepath.Join(filepath.Dir(sourceFile), "..", "..", "..", "..", "..",
		"Control Plane", "config", "plane-service-principals.json")
	raw, err := os.ReadFile(policyPath)
	if err != nil {
		t.Fatalf("read the Control Plane principal policy: %v", err)
	}
	var policy map[string]principalPolicyRow
	if err := json.Unmarshal(raw, &policy); err != nil {
		t.Fatalf("decode the principal policy: %v", err)
	}

	row, found := policy[defaultServiceID]
	if !found {
		t.Fatalf("the principal policy has no %q row: Auth Core refuses every token sandbox-manager asks for, so every thread-scoped call fails closed as UNAVAILABLE", defaultServiceID)
	}
	if row.Tenancy != "directory" {
		t.Fatalf("tenancy = %q, want \"directory\": sandbox-manager serves every organization", row.Tenancy)
	}
	if !slices.Equal(row.Audiences, []string{"session-core"}) {
		t.Fatalf("audiences = %q, want exactly [session-core]", row.Audiences)
	}
	if row.ConsumerEnv != serviceCredentialEnv {
		t.Fatalf("consumerEnv = %q, want %q, the credential the switch reads", row.ConsumerEnv, serviceCredentialEnv)
	}
	if !slices.Equal(row.Scopes, []string{threadowner.Scope}) {
		t.Fatalf("scopes = %q, want exactly %q", row.Scopes, threadowner.Scope)
	}
	if len(row.ScopesByAudience) != 1 || !slices.Equal(row.ScopesByAudience["session-core"], []string{threadowner.Scope}) {
		t.Fatalf("scopesByAudience = %v, want session-core alone with %q", row.ScopesByAudience, threadowner.Scope)
	}
	if row.RetentionByAudience["session-core"] != "persistent" {
		t.Fatalf("retentionByAudience[session-core] = %q, want persistent: the lookup guards a durable thread workspace", row.RetentionByAudience["session-core"])
	}
	for name, other := range policy {
		if name == defaultServiceID {
			continue
		}
		if slices.Contains(other.Scopes, threadowner.Scope) {
			t.Fatalf("principal %q holds %q: only sandbox-manager may ask session-core for thread ownership", name, threadowner.Scope)
		}
		for audience, scopes := range other.ScopesByAudience {
			if slices.Contains(scopes, threadowner.Scope) {
				t.Fatalf("principal %q holds %q on %q: only sandbox-manager may ask session-core for thread ownership", name, threadowner.Scope, audience)
			}
		}
	}
}
