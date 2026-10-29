package api

import (
	"encoding/json"
	"io"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/secret"
	"github.com/triodelab/studio-plane/planeclient"
)

func TestPlaneConnectionsAndDraftsRequireCoreAuthentication(t *testing.T) {
	planes, err := planeclient.New(planeclient.Config{Core: "anbud-core"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	app := NewServer(ServerConfig{APIKey: secret.Value("test-key"), Planes: planes})
	const base = "/api/v1/anbud"
	cases := []struct {
		name, method, path, key, body string
		status                        int
	}{
		{"missing key", "GET", base + "/plane-connections", "", "", 401},
		{"wrong key", "GET", base + "/plane-connections", "wrong-key", "", 401},
		{"unconfigured connections", "GET", base + "/plane-connections", "test-key", "", 200},
		{"foreign query", "GET", base + "/plane-connections?orgId=foreign", "test-key", "", 400},
		{"unauthenticated draft", "POST", base + "/assistant/drafts", "", `{"prompt":"Draft a title"}`, 401},
		{"unconfigured draft", "POST", base + "/assistant/drafts", "test-key", `{"prompt":"Draft a title"}`, 503},
		{"foreign organization", "POST", base + "/assistant/drafts", "test-key", `{"prompt":"Draft a title","orgId":"foreign"}`, 400},
		{"tool injection", "POST", base + "/assistant/drafts", "test-key", `{"prompt":"Draft a title","tools":["publish"]}`, 400},
		{"duplicate prompt", "POST", base + "/assistant/drafts", "test-key", `{"prompt":"a","prompt":"b"}`, 400},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
			r.Header.Set("Content-Type", "application/json")
			r.Header.Set("X-Internal-API-Key", tc.key)
			response, err := app.Test(r)
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			body, err := io.ReadAll(response.Body)
			if err != nil {
				t.Fatal(err)
			}
			if response.StatusCode != tc.status {
				t.Fatalf("status=%d want=%d body=%s", response.StatusCode, tc.status, body)
			}
			if tc.status == 200 {
				var out struct {
					Data planeclient.Connections `json:"data"`
				}
				if err := json.Unmarshal(body, &out); err != nil {
					t.Fatal(err)
				}
				if out.Data.Quarry.Configured || out.Data.Model.Configured || out.Data.Quarry.Status != "disabled" || out.Data.Model.Status != "disabled" {
					t.Fatalf("fabricated connection: %+v", out.Data)
				}
				if response.Header.Get("Cache-Control") != "no-store" {
					t.Fatal("configuration response is cacheable")
				}
			}
		})
	}
}
