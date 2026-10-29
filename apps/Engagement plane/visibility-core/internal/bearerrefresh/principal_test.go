package bearerrefresh

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"os"
	"strings"
	"testing"
	"time"
)

func principalToken(t *testing.T, now time.Time, subject, service string) string {
	t.Helper()
	parts := strings.Split(tokenFor(t, now, "org-aquatiq"), ".")
	contents, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		t.Fatal(err)
	}
	var claims map[string]any
	if err := json.Unmarshal(contents, &claims); err != nil {
		t.Fatal(err)
	}
	updated, err := json.Marshal(func() map[string]any {
		result := make(map[string]any, len(claims))
		for key, value := range claims {
			result[key] = value
		}
		result["sub"], result["service_id"] = subject, service
		return result
	}())
	if err != nil {
		t.Fatal(err)
	}
	return parts[0] + "." + base64.RawURLEncoding.EncodeToString(updated) + "." + parts[2]
}

func TestRefreshBindsConfiguredDeploymentPrincipal(t *testing.T) {
	for _, service := range []string{"", "marketing-core", "marketing-core-triodelab"} {
		t.Run("service="+service, func(t *testing.T) {
			expected := service
			if expected == "" {
				expected = "marketing-core"
			}
			var now time.Time
			cfg, client, fixed := fixture(t, func(w http.ResponseWriter, request *http.Request) {
				if request.Header.Get("x-service-id") != expected {
					t.Error("issuance did not use the configured deployment principal")
				}
				var body struct {
					Reason string `json:"reason"`
				}
				if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
					t.Error(err)
				}
				if body.Reason != "Read marketing data for Studio Pulse" {
					t.Error("audit reason is not neutral Studio marketing read intent")
				}
				_ = json.NewEncoder(w).Encode(map[string]any{
					"token":     principalToken(t, now, "service:"+expected, "service:"+expected),
					"expiresAt": now.Add(300 * time.Second), "audience": "ingestion",
					"issuer": "http://localhost:3011/api/convex-auth",
				})
			})
			now = fixed
			cfg.ServiceID = service
			refresher, err := New(cfg, client, func() time.Time { return now })
			if err != nil {
				t.Fatal(err)
			}
			if cfg.ServiceID != service {
				t.Fatal("caller deployment configuration was mutated")
			}
			if _, err := refresher.Refresh(context.Background()); err != nil {
				t.Fatal(err)
			}
			if _, err := refresher.Check(); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestRefresherRejectsOtherPrincipalWithoutReplacingToken(t *testing.T) {
	for _, identity := range []struct{ name, configured, subject, service string }{
		{"default principal", "marketing-core-triodelab", "service:marketing-core", "service:marketing-core"},
		{"subject mismatch", "marketing-core-triodelab", "service:marketing-core", "service:marketing-core-triodelab"},
		{"service claim mismatch", "marketing-core-triodelab", "service:marketing-core-triodelab", "service:marketing-core"},
		{"default rejects custom principal", "", "service:marketing-core-triodelab", "service:marketing-core-triodelab"},
	} {
		t.Run(identity.name, func(t *testing.T) {
			var now time.Time
			cfg, client, fixed := fixture(t, func(w http.ResponseWriter, request *http.Request) {
				_ = json.NewEncoder(w).Encode(map[string]any{
					"token":     principalToken(t, now, identity.subject, identity.service),
					"expiresAt": now.Add(300 * time.Second), "audience": "ingestion",
					"issuer": "http://localhost:3011/api/convex-auth",
				})
			})
			now = fixed
			cfg.ServiceID = identity.configured
			refresher, err := New(cfg, client, func() time.Time { return now })
			if err != nil {
				t.Fatal(err)
			}
			original := principalToken(t, now, identity.subject, identity.service)
			if err := os.WriteFile(cfg.TokenFile, []byte(original), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := refresher.Check(); err == nil {
				t.Fatal("wrong-principal token was considered ready")
			}
			if _, err := refresher.Refresh(context.Background()); err == nil {
				t.Fatal("wrong-principal issuance response was accepted")
			}
			contents, err := os.ReadFile(cfg.TokenFile)
			if err != nil || string(contents) != original {
				t.Fatal("existing token changed after refused issuance")
			}
		})
	}
}

func TestDeploymentPrincipalValidation(t *testing.T) {
	cfg, client, now := fixture(t, func(w http.ResponseWriter, r *http.Request) {})
	for _, service := range []string{" ", " marketing-core", "marketing-core ", "service:marketing-core", "../marketing-core", "marketing\ncore", "marketing\tcore", "marketing/core", "marketing.core", "-marketing", "markéting", strings.Repeat("a", 129)} {
		t.Run(service, func(t *testing.T) {
			candidate := cfg
			candidate.ServiceID = service
			if _, err := New(candidate, client, func() time.Time { return now }); err == nil {
				t.Fatal("unsafe deployment principal accepted")
			}
		})
	}
	for _, service := range []string{"a", "marketing_core-2", strings.Repeat("a", 128)} {
		candidate := cfg
		candidate.ServiceID = service
		if _, err := New(candidate, client, func() time.Time { return now }); err != nil {
			t.Fatalf("safe principal refused: %v", err)
		}
	}
}
