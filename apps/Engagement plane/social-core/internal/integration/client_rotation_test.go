package integration

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
)

func writeRotatingBearer(t *testing.T, path, org string, expiry int64, nonce string) string {
	t.Helper()
	claims, _ := json.Marshal(map[string]any{"aud": "ingestion", "org_id": org, "sub": "service:social-core", "service_id": "service:social-core", "principal_type": "service", "exp": expiry, "scopes": []string{"integration:read"}, "jti": nonce})
	token := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256"}`)) + "." + base64.RawURLEncoding.EncodeToString(claims) + ".c2ln"
	if err := os.WriteFile(path, []byte(token), 0600); err != nil {
		t.Fatal(err)
	}
	return token
}

func TestTenantFileReloadsForListsAndDirectActionsWithoutFallback(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bearer")
	want := writeRotatingBearer(t, path, "org-1", time.Now().Add(time.Hour).Unix(), "first")
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.Header.Get("Authorization") != "Bearer "+want {
			t.Error("request used stale or fallback credential")
		}
		if r.URL.Path == "/api/v1/connections" {
			_, _ = w.Write([]byte(`{"success":true,"data":{"connections":[]}}`))
			return
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"meta","operation":"meta.ads.insights","result":{}}}}`))
	}))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "legacy-must-not-be-used", TenantBearerTokenFile: path, TenantOrganizationID: "org-1"})
	if _, err := client.ListSocialAccounts(context.Background(), "org-1"); err != nil {
		t.Fatal(err)
	}
	want = writeRotatingBearer(t, path, "org-1", time.Now().Add(time.Hour).Unix(), "second")
	if _, err := client.ExecuteAction(context.Background(), social.ActionRequest{ConnectionID: "conn-1", Operation: "meta.ads.insights"}); err != nil {
		t.Fatal(err)
	}
	if calls != 2 {
		t.Fatalf("calls=%d", calls)
	}
	for _, invalid := range []string{"expired", "wrong-org", "public-permissions", "missing"} {
		t.Run(invalid, func(t *testing.T) {
			expiry, org := time.Now().Add(time.Hour).Unix(), "org-1"
			if invalid == "expired" {
				expiry = time.Now().Add(-time.Minute).Unix()
			}
			if invalid == "wrong-org" {
				org = "org-other"
			}
			writeRotatingBearer(t, path, org, expiry, invalid)
			if invalid == "public-permissions" {
				if err := os.Chmod(path, 0644); err != nil {
					t.Fatal(err)
				}
				defer os.Chmod(path, 0600)
			}
			if invalid == "missing" {
				if err := os.Remove(path); err != nil {
					t.Fatal(err)
				}
			}
			if _, err := client.ListSocialAccounts(context.Background(), "org-1"); err == nil {
				t.Fatal("invalid file allowed listing")
			}
			if _, err := client.ExecuteAction(context.Background(), social.ActionRequest{ConnectionID: "conn-1", Operation: "meta.ads.insights"}); err == nil {
				t.Fatal("invalid file allowed action")
			}
			if calls != 2 {
				t.Fatal("invalid file made HTTP call or fell back")
			}
		})
	}
}
