package pagespeed

import (
	"encoding/json"
	"strings"
	"testing"
)

var expectedCheckIDs = []string{"document-title", "meta-description", "http-status-code", "is-crawlable", "robots-txt", "canonical", "hreflang", "link-text", "crawlable-anchors", "image-alt", "structured-data"}

func reportWithAudits(t *testing.T, audits string) *Report {
	t.Helper()
	raw := strings.Replace(fixture, `"audits":{`, `"audits":{`+audits, 1)
	report, reason := decodeReport([]byte(raw), "https://aquatiq.com/", "mobile")
	if reason != "" || report == nil || report.LCPMs == nil || *report.LCPMs != 2345 {
		t.Fatalf("diagnostics broke numeric report: %+v %q", report, reason)
	}
	return report
}

func TestChecksAreFixedOrderedAndSanitized(t *testing.T) {
	report := reportWithAudits(t, `"document-title":{"scoreDisplayMode":"binary","score":1,"title":"PRIVATE_TITLE","description":"PRIVATE_DESCRIPTION","details":{"url":"https://secret.example/"}},"meta-description":{"scoreDisplayMode":"binary","score":0},"unknown-check":{"scoreDisplayMode":"binary","score":0,"debugString":"PRIVATE_DEBUG"},`)
	if len(report.Checks) != len(expectedCheckIDs) {
		t.Fatalf("checks = %+v", report.Checks)
	}
	for i, id := range expectedCheckIDs {
		got := report.Checks[i]
		if got.ID != id {
			t.Fatalf("check %d = %q, want %q", i, got.ID, id)
		}
		if i > 1 && (got.Status != "not_reported" || got.Score != nil) {
			t.Fatalf("absent check invented outcome: %+v", got)
		}
	}
	if report.Checks[0].Status != "passed" || *report.Checks[0].Score != 1 || report.Checks[1].Status != "failed" || *report.Checks[1].Score != 0 {
		t.Fatalf("binary outcomes = %+v", report.Checks[:2])
	}
	raw, err := json.Marshal(report)
	if err != nil {
		t.Fatal(err)
	}
	for _, forbidden := range []string{"PRIVATE_", "secret.example", "unknown-check", `"description":`, `"debugString":`, `"details":`} {
		if strings.Contains(string(raw), forbidden) {
			t.Fatalf("provider data escaped: %s", raw)
		}
	}
	if !strings.Contains(string(raw), `"id":"structured-data","status":"not_reported","score":null`) {
		t.Fatalf("missing score must remain explicit null: %s", raw)
	}
}

func TestCheckModesAndMalformedValuesDoNotRejectReport(t *testing.T) {
	for _, tc := range []struct{ name, audit, status string }{
		{"not applicable", `{"scoreDisplayMode":"notApplicable","score":null}`, "not_applicable"},
		{"manual", `{"scoreDisplayMode":"manual","score":null}`, "manual"},
		{"informative", `{"scoreDisplayMode":"informative","score":null}`, "informative"},
		{"error", `{"scoreDisplayMode":"error","score":null,"errorMessage":"PRIVATE_ERROR"}`, "error"},
		{"missing mode", `{"score":1}`, "not_reported"},
		{"missing score", `{"scoreDisplayMode":"binary"}`, "not_reported"},
		{"manual missing score", `{"scoreDisplayMode":"manual"}`, "not_reported"},
		{"fractional", `{"scoreDisplayMode":"binary","score":0.5}`, "not_reported"},
		{"binary null", `{"scoreDisplayMode":"binary","score":null}`, "not_reported"},
		{"negative", `{"scoreDisplayMode":"binary","score":-1}`, "not_reported"},
		{"oversized", `{"scoreDisplayMode":"binary","score":2}`, "not_reported"},
		{"numeric mode", `{"scoreDisplayMode":"numeric","score":1}`, "not_reported"},
		{"manual scored", `{"scoreDisplayMode":"manual","score":1}`, "not_reported"},
		{"unknown mode", `{"scoreDisplayMode":"unknown","score":0}`, "not_reported"},
		{"score string", `{"scoreDisplayMode":"binary","score":"1"}`, "not_reported"},
		{"score boolean", `{"scoreDisplayMode":"binary","score":true}`, "not_reported"},
		{"score object", `{"scoreDisplayMode":"binary","score":{}}`, "not_reported"},
		{"score overflow", `{"scoreDisplayMode":"binary","score":1e999}`, "not_reported"},
		{"mode object", `{"scoreDisplayMode":{},"score":1}`, "not_reported"},
		{"audit string", `"bad"`, "not_reported"},
		{"audit array", `[]`, "not_reported"},
		{"audit boolean", `true`, "not_reported"},
		{"audit null", `null`, "not_reported"},
		{"bad irrelevant scalar", `{"scoreDisplayMode":"binary","score":1,"numericValue":"bad"}`, "passed"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			report := reportWithAudits(t, `"canonical":`+tc.audit+`,`)
			got := report.Checks[5]
			if got.Status != tc.status || tc.status != "passed" && got.Score != nil {
				t.Fatalf("check = %+v, want %s", got, tc.status)
			}
		})
	}
}

func TestMalformedProjectedNumericMetricsStillFail(t *testing.T) {
	for _, value := range []string{`"bad"`, `true`, `{}`, `-1`} {
		raw := strings.Replace(fixture, `"numericValue":2345`, `"numericValue":`+value, 1)
		if report, reason := decodeReport([]byte(raw), "https://aquatiq.com/", "mobile"); report != nil || reason != "audit_failed" {
			t.Fatalf("malformed numeric metric admitted: %+v %q", report, reason)
		}
	}
}

func TestCheckSnapshotsOwnSliceAndScores(t *testing.T) {
	score := 1.0
	source := Snapshot{Report: &Report{Checks: []Check{{ID: "canonical", Status: "passed", Score: &score}, {ID: "robots-txt", Status: "not_reported"}}}}
	first := clone(source)
	first.Report.Checks[0].Status = "failed"
	*first.Report.Checks[0].Score = 0
	first.Report.Checks[1].ID = "changed"
	if source.Report.Checks[0].Status != "passed" || *source.Report.Checks[0].Score != 1 || source.Report.Checks[1].ID != "robots-txt" {
		t.Fatalf("clone shares diagnostics: %+v", source.Report.Checks)
	}
	if cloned := clone(Snapshot{Report: &Report{}}); cloned.Report.Checks != nil {
		t.Fatal("nil checks changed shape")
	}
}
