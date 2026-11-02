package bearerrefresh

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

func profileToken(t *testing.T, now time.Time, audience string, scopes []string, changes map[string]any) string {
	t.Helper()
	claims := map[string]any{"iss": "http://localhost:3011/api/convex-auth", "aud": audience, "sub": "service:marketing-core", "service_id": "service:marketing-core", "principal_type": "service", "org_id": "org-aquatiq", "scopes": scopes, "iat": now.Unix(), "nbf": now.Add(-5 * time.Second).Unix(), "exp": now.Add(300 * time.Second).Unix()}
	for key, value := range changes {
		claims[key] = value
	}
	header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256","kid":"test-key"}`))
	payload, err := json.Marshal(claims)
	if err != nil {
		t.Fatal(err)
	}
	return header + "." + base64.RawURLEncoding.EncodeToString(payload) + ".synthetic-signature"
}

func TestCapabilityProfilesIssueOnlyFixedAudiencesAndScopes(t *testing.T) {
	for _, test := range []struct {
		capability, audience, path string
		scopes                     []string
	}{
		{"integration-read", "ingestion", "/api/ingestion/internal-token", []string{"integration:read"}},
		{"integration-readwrite", "ingestion", "/api/ingestion/internal-token", []string{"integration:read", "integration:write"}},
		{"quarry-discovery", "quarry", "/api/quarry/internal-token", []string{"map:read"}},
		{"quarry-capture", "quarry", "/api/quarry/internal-token", []string{"map:read", "scrape:read", "scrape:write"}},
		{"model-draft", "model-gateway", "/api/model-plane/internal-token", []string{"models:invoke"}},
		{"inference-draft", "inference-core", "/api/inference-core/internal-token", []string{"inference:invoke"}},
	} {
		t.Run(test.capability, func(t *testing.T) {
			var now time.Time
			cfg, client, fixed := fixture(t, func(w http.ResponseWriter, request *http.Request) {
				if request.Method != http.MethodPost || request.URL.Path != test.path {
					t.Errorf("unexpected issuance route %s", request.URL.Path)
				}
				var body struct {
					OrgID  string   `json:"orgId"`
					Scopes []string `json:"scopes"`
					Reason string   `json:"reason"`
				}
				if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
					t.Error(err)
				}
				if body.OrgID != "org-aquatiq" || !reflect.DeepEqual(body.Scopes, test.scopes) || body.Reason == "" {
					t.Error("wrong fixed issuance policy")
				}
				_ = json.NewEncoder(w).Encode(map[string]any{"token": profileToken(t, now, test.audience, test.scopes, nil), "expiresAt": now.Add(300 * time.Second), "audience": test.audience, "issuer": "http://localhost:3011/api/convex-auth"})
			})
			now, cfg.Capability = fixed, test.capability
			refresher, err := New(cfg, client, func() time.Time { return now })
			if err != nil {
				t.Fatal(err)
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

func TestProfilesRefusePrivilegeExpansionAndCrossProfileTokenReuse(t *testing.T) {
	for _, changes := range []map[string]any{
		{"aud": "ingestion"}, {"scopes": []string{"map:read", "scrape:write"}}, {"scopes": []string{"map:read", "map:read"}},
		{"scopes": []string{}}, {"org_id": "other-org"}, {"user_id": "someone"}, {"principal_type": "user"},
		{"sub": "service:other"}, {"service_id": "service:other"}, {"exp": int64(1)},
	} {
		t.Run("reject", func(t *testing.T) {
			var now time.Time
			cfg, client, fixed := fixture(t, func(w http.ResponseWriter, request *http.Request) {
				_ = json.NewEncoder(w).Encode(map[string]any{"token": profileToken(t, now, "quarry", []string{"map:read"}, changes), "expiresAt": now.Add(300 * time.Second), "audience": "quarry", "issuer": "http://localhost:3011/api/convex-auth"})
			})
			now, cfg.Capability = fixed, "quarry-discovery"
			refresher, err := New(cfg, client, func() time.Time { return now })
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(cfg.TokenFile, []byte("existing"), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := refresher.Refresh(context.Background()); err == nil {
				t.Fatal("invalid profile issuance accepted")
			}
			contents, _ := os.ReadFile(cfg.TokenFile)
			if string(contents) != "existing" {
				t.Fatal("refused issuance replaced previous token")
			}
			if err := os.WriteFile(cfg.TokenFile, []byte(profileToken(t, now, "quarry", []string{"map:read"}, changes)), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := refresher.Check(); err == nil {
				t.Fatal("wrong binding considered ready")
			}
		})
	}
}

func TestUnknownCapabilityFailsClosed(t *testing.T) {
	cfg, client, now := fixture(t, func(w http.ResponseWriter, request *http.Request) { t.Error("issuance must not happen") })
	for _, capability := range []string{"quarry", "model", "integration-read ", " quarry-discovery", "*", strings.Repeat("a", 100)} {
		candidate := cfg
		candidate.Capability = capability
		if _, err := New(candidate, client, func() time.Time { return now }); err == nil {
			t.Fatalf("accepted unknown capability %s", capability)
		}
	}
}

func TestProfileTokenRequiresFreshServiceOnlyRS256Claims(t *testing.T) {
	now := time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC)
	profile, _ := profileFor("quarry-discovery")
	for _, changes := range []map[string]any{
		{"iat": now.Add(-2 * time.Hour).Unix()}, {"iat": now.Add(time.Minute).Unix()},
		{"nbf": now.Add(time.Second).Unix()}, {"exp": now.Add(2 * time.Hour).Unix()},
		{"user_id": ""}, {"user_id": nil},
	} {
		if _, _, err := tokenExpiryForProfile(profileToken(t, now, "quarry", []string{"map:read"}, changes), "org-aquatiq", "marketing-core", now, profile); err == nil {
			t.Fatal("invalid freshness or ambiguous principal accepted")
		}
	}
	valid := profileToken(t, now, "quarry", []string{"map:read"}, nil)
	parts := strings.Split(valid, ".")
	for _, header := range []string{`{"alg":"HS256","kid":"test-key"}`, `{"alg":"none","kid":"test-key"}`, `{"alg":"RS256"}`} {
		token := base64.RawURLEncoding.EncodeToString([]byte(header)) + "." + parts[1] + "." + parts[2]
		if _, _, err := tokenExpiryForProfile(token, "org-aquatiq", "marketing-core", now, profile); err == nil {
			t.Fatal("invalid RS256 header accepted")
		}
	}
	capture, _ := profileFor("quarry-capture")
	if _, _, err := tokenExpiryForProfile(profileToken(t, now, "quarry", []string{"scrape:write", "map:read", "scrape:read"}, nil), "org-aquatiq", "marketing-core", now, capture); err != nil {
		t.Fatal("equivalent scope order refused")
	}
}
