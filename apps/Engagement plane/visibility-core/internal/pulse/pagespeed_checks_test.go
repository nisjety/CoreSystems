package pulse

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestPageSpeedChecksValidateStatusWithoutInventedPasses(t *testing.T) {
	for _, status := range []string{"passed", "failed", "not_applicable", "manual", "informative", "error", "not_reported"} {
		t.Run(status, func(t *testing.T) {
			var score *float64
			if status == "passed" || status == "failed" {
				value := 0.0
				if status == "passed" {
					value = 1
				}
				score = &value
			}
			checks := []PageSpeedCheck{{ID: "canonical", Status: status, Score: score}}
			if !validPageSpeedChecks(checks) {
				t.Fatal("valid check rejected")
			}
		})
	}
	zero, one, half := 0.0, 1.0, 0.5
	for _, checks := range [][]PageSpeedCheck{
		{{ID: "private-audit", Status: "passed", Score: &one}},
		{{ID: "canonical", Status: "passed"}},
		{{ID: "canonical", Status: "failed", Score: &one}},
		{{ID: "canonical", Status: "passed", Score: &zero}},
		{{ID: "canonical", Status: "passed", Score: &half}},
		{{ID: "canonical", Status: "manual", Score: &zero}},
		{{ID: "canonical", Status: "not_reported", Score: &one}},
		{{ID: "canonical", Status: "numeric", Score: &half}},
		{{ID: "canonical", Status: "not_reported"}, {ID: "canonical", Status: "not_reported"}},
	} {
		if validPageSpeedChecks(checks) {
			t.Fatalf("invalid check accepted: %+v", checks)
		}
	}
	if !validPageSpeedChecks(nil) {
		t.Fatal("older cached reports must remain readable")
	}
	if validPageSpeedChecks(make([]PageSpeedCheck, 12)) {
		t.Fatal("over-limit check list accepted")
	}
}

func TestPageSpeedChecksCrossFixedBoundaryAndStripProviderDetails(t *testing.T) {
	checks := `,"checks":[{"id":"document-title","status":"passed","score":1,"details":{"token":"private-canary"}},{"id":"canonical","status":"failed","score":0},{"id":"structured-data","status":"manual","score":null}]`
	body := strings.Replace(pageSpeedComplete, `"lighthouseVersion":"12.8.2"`, `"lighthouseVersion":"12.8.2"`+checks, 1)
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/insights/pagespeed" {
			t.Error("unexpected outbound route")
		}
		_, _ = w.Write([]byte(`{"data":` + body + `}`))
	}))
	defer provider.Close()
	svc := newTestService(t, provider.URL, "")
	svc.cfg.GoogleConnectionID = ""
	got := svc.GetPageSpeed(context.Background(), "https://aquatiq.com/", "mobile")
	if got.Status != "complete" || got.Report == nil || len(got.Report.Checks) != 3 {
		t.Fatalf("checks not projected: %+v", got)
	}
	raw, err := json.Marshal(got)
	if err != nil || strings.Contains(string(raw), "private-canary") || strings.Contains(string(raw), "details") {
		t.Fatal("provider details escaped")
	}
	if got.Report.Checks[2].Score != nil {
		t.Fatal("manual check invented a score")
	}
}

func TestPageSpeedProjectionRejectsMalformedChecks(t *testing.T) {
	for _, checks := range []string{
		`[{"id":"canonical","status":"passed","score":0}]`,
		`[{"id":"unknown","status":"passed","score":1}]`,
		`[{"id":"canonical","status":"not_reported","score":null},{"id":"canonical","status":"not_reported","score":null}]`,
	} {
		var value PageSpeedProjection
		_ = json.Unmarshal([]byte(pageSpeedComplete), &value)
		if json.Unmarshal([]byte(checks), &value.Report.Checks) != nil {
			t.Fatal("bad test fixture")
		}
		if validPageSpeedProjection(value) {
			t.Fatal("unsafe checks accepted by report validator")
		}
	}
}
