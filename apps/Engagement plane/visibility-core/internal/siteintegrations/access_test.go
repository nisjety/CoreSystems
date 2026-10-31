package siteintegrations

import (
	"net/http"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func accessRecord() websites.Record {
	return websites.Record{Website: websites.Website{Site: "https://triodelab.no/", Integrations: websites.Integrations{SearchConsole: websites.Mapping{Status: "connected"}, GA4: websites.Mapping{Status: "connected"}, Clarity: websites.Mapping{ProjectID: "project123"}}}, GSCConnectionID: "conn-gsc", GA4ConnectionID: "conn-ga4"}
}
func TestCurrentAccessUsesOnlyOrgScopedMetadataAndFailsClosed(t *testing.T) {
	for _, bad := range []string{"", "inactive", "foreign_org", "missing_scope", "missing_capability", "foreign_id", "deleted", "clarity_rebound", "clarity_invalid"} {
		t.Run(bad, func(t *testing.T) {
			var calls atomic.Int64
			b := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				if r.Method != "GET" || strings.Contains(r.URL.Path, "/actions") {
					t.Error("authority check attempted provider action")
				}
				if r.URL.Path == "/api/v1/insights/clarity/binding" {
					project, version := "project123", strings.Repeat("a", 64)
					if bad == "clarity_rebound" {
						project = "another123"
					}
					if bad == "clarity_invalid" {
						version = "short"
					}
					writeFixture(w, map[string]any{"success": true, "data": map[string]any{"site": "https://triodelab.no/", "configured": true, "projectId": project, "version": version}})
					return
				}
				id := strings.TrimPrefix(r.URL.Path, "/api/v1/connections/")
				scope, capability := gscScope, "search_console.read"
				if id == "conn-ga4" {
					scope, capability = gaScope, "analytics.read"
				}
				item := map[string]any{"id": id, "organizationId": "org-test", "providerKey": "google", "status": "active", "scopes": []string{scope}, "capabilities": []string{capability}, "deletedAt": nil}
				if id == "conn-gsc" {
					switch bad {
					case "inactive":
						item["status"] = "error"
					case "foreign_org":
						item["organizationId"] = "other-org"
					case "missing_scope":
						item["scopes"] = []string{}
					case "missing_capability":
						item["capabilities"] = []string{}
					case "foreign_id":
						item["id"] = "other-id"
					case "deleted":
						item["deletedAt"] = "2026-10-23T00:00:00Z"
					}
				}
				writeFixture(w, map[string]any{"success": true, "data": map[string]any{"connection": item}})
			})
			got := b.CurrentAccess(t.Context(), "org-test", accessRecord())
			wantGSC := bad == "" || strings.HasPrefix(bad, "clarity_")
			if got.SearchConsole != wantGSC || !got.GA4 || got.Clarity == strings.HasPrefix(bad, "clarity_") {
				t.Fatalf("bad %s: %+v", bad, got)
			}
			if calls.Load() != 3 {
				t.Fatal("unbounded metadata reads")
			}
		})
	}
	b := New(Config{})
	got := b.CurrentAccess(t.Context(), "org-test", accessRecord())
	if got.SearchConsole || got.GA4 || got.Clarity {
		t.Fatal("unconfigured authority accepted")
	}
}
