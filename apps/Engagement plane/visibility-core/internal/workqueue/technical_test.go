package workqueue

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type technicalStub struct {
	*searchStub
	report pulse.PageSpeedProjection
	calls  int
	during func()
}

func (s *technicalStub) GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection {
	s.calls++
	if s.during != nil {
		s.during()
	}
	return s.report
}
func technicalService(t *testing.T) (*Service, *websites.Service, websites.Website, *technicalStub, string) {
	s, registry, w, search, path := testService(t)
	source := &technicalStub{searchStub: search, report: pulse.PageSpeedProjection{Site: w.Site, Source: "google_pagespeed", Strategy: "desktop", Status: "complete", Report: &pulse.PageSpeedReport{FetchTime: "2026-10-22T10:00:00Z", LighthouseVersion: "13.0.0", Checks: []pulse.PageSpeedCheck{{ID: "structured-data", Status: "manual"}}}}}
	s.source = source
	return s, registry, w, source, path
}
func saveTechnical(t *testing.T, s *Service, w websites.Website, source *technicalStub) Snapshot {
	t.Helper()
	value, created, err := s.SaveTechnical(t.Context(), w.ID, w.Site, w.Revision, "desktop", "structured-data", source.report.Report.FetchTime)
	if err != nil || !created {
		t.Fatalf("save technical created=%v err=%v", created, err)
	}
	return value
}
func TestTechnicalEvidenceVerificationPersistsWithoutClosingTask(t *testing.T) {
	s, registry, w, source, path := technicalService(t)
	value := saveTechnical(t, s, w, source)
	task := value.Tasks[0]
	raw, _ := json.Marshal(task.Evidence)
	for _, unwanted := range []string{"clicks", "impressions", "period", "opportunityType", "position", "ctr"} {
		if strings.Contains(string(raw), `"`+unwanted+`"`) {
			t.Fatalf("invented GSC field %s: %s", unwanted, raw)
		}
	}
	if !strings.Contains(string(raw), `"checkScore":null`) || task.Evidence.Kind != "pagespeed_check" {
		t.Fatalf("evidence=%s", raw)
	}
	if _, created, err := s.SaveTechnical(t.Context(), w.ID, w.Site, w.Revision, "desktop", "structured-data", source.report.Report.FetchTime); err != nil || created {
		t.Fatalf("dedup %v %v", created, err)
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); !errors.Is(err, ErrConflict) {
		t.Fatalf("same report accepted %v", err)
	}
	pass := 1.0
	source.report.Report.FetchTime = "2026-10-22T11:00:00.123Z"
	source.report.Report.Checks[0] = pulse.PageSpeedCheck{ID: "structured-data", Status: "passed", Score: &pass}
	value, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1)
	if err != nil || value.Tasks[0].Status != "todo" || value.Tasks[0].Revision != 2 || value.Tasks[0].Verification.Status != "passed" || value.Activity[0].Kind != "verified" || value.Summary.Done != 0 {
		t.Fatalf("verified=%+v err=%v", value, err)
	}
	*value.Tasks[0].Verification.Score = 0
	*value.Activity[0].Verification.Score = 0
	unchanged, _ := s.List(w.ID, w.Site)
	if *unchanged.Tasks[0].Verification.Score != 1 || *unchanged.Activity[0].Verification.Score != 1 {
		t.Fatal("snapshot pointer alias")
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 2); !errors.Is(err, ErrConflict) {
		t.Fatalf("repeated verification %v", err)
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); !errors.Is(err, ErrConflict) {
		t.Fatalf("stale task %v", err)
	}
	source.report.Report.FetchTime = "2026-10-22T11:30:00Z"
	source.report.Report.Checks[0] = pulse.PageSpeedCheck{ID: "structured-data", Status: "manual"}
	value, err = s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 2)
	if err != nil || value.Tasks[0].Verification.Status != "manual" || len(value.Activity) != 3 || value.Activity[1].Verification.Status != "passed" {
		t.Fatalf("history=%+v %v", value, err)
	}
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	restored, err := New(repo, registry, source, nil).List(w.ID, w.Site)
	if err != nil || restored.Tasks[0].Revision != 3 || restored.Tasks[0].Verification.Status != "manual" {
		t.Fatalf("reopen %+v %v", restored, err)
	}
}
func TestTechnicalRejectsUntrustedOrStaleProviderEvidence(t *testing.T) {
	for _, tc := range []struct {
		name                string
		change              func(*technicalStub)
		strategy, id, stamp string
		want                error
	}{
		{"not complete", func(s *technicalStub) { s.report.Status = "running" }, "desktop", "structured-data", "2026-10-22T10:00:00Z", ErrSourceUnavailable},
		{"wrong source", func(s *technicalStub) { s.report.Source = "other" }, "desktop", "structured-data", "2026-10-22T10:00:00Z", ErrSourceUnavailable},
		{"wrong site", func(s *technicalStub) { s.report.Site = "https://other.no/" }, "desktop", "structured-data", "2026-10-22T10:00:00Z", ErrSourceUnavailable},
		{"wrong strategy", func(s *technicalStub) { s.report.Strategy = "mobile" }, "desktop", "structured-data", "2026-10-22T10:00:00Z", ErrSourceUnavailable},
		{"old UI", nil, "desktop", "structured-data", "2026-10-21T10:00:00Z", ErrConflict},
		{"missing check", nil, "desktop", "canonical", "2026-10-22T10:00:00Z", ErrNotFound},
		{"invalid check", nil, "desktop", "unknown", "2026-10-22T10:00:00Z", ErrInvalid},
		{"invalid strategy", nil, "tablet", "structured-data", "2026-10-22T10:00:00Z", ErrInvalid},
		{"passed", func(s *technicalStub) {
			v := 1.0
			s.report.Report.Checks[0].Status = "passed"
			s.report.Report.Checks[0].Score = &v
		}, "desktop", "structured-data", "2026-10-22T10:00:00Z", ErrInvalid},
		{"bad score", func(s *technicalStub) { v := 0.3; s.report.Report.Checks[0].Score = &v }, "desktop", "structured-data", "2026-10-22T10:00:00Z", ErrSourceUnavailable},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s, _, w, source, _ := technicalService(t)
			if tc.change != nil {
				tc.change(source)
			}
			if _, _, err := s.SaveTechnical(t.Context(), w.ID, w.Site, w.Revision, tc.strategy, tc.id, tc.stamp); !errors.Is(err, tc.want) {
				t.Fatalf("got %v want %v", err, tc.want)
			}
			got, _ := s.List(w.ID, w.Site)
			if len(got.Tasks) != 0 {
				t.Fatal("unverified task stored")
			}
		})
	}
}
func TestTechnicalRechecksBindingsAndTaskRevisionAroundProviderRead(t *testing.T) {
	s, registry, w, source, _ := technicalService(t)
	value := saveTechnical(t, s, w, source)
	task := value.Tasks[0]
	source.report.Report.FetchTime = "2026-10-22T11:00:00Z"
	source.during = func() {
		_, err := s.Change(w.ID, w.Site, w.Revision, task.ID, 1, "in_progress")
		if err != nil {
			t.Error(err)
		}
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); !errors.Is(err, ErrConflict) {
		t.Fatalf("concurrent task change %v", err)
	}
	source.during = func() {
		_, err := registry.Change(t.Context(), w.ID, "https://agenci.no/", w.Revision)
		if err != nil {
			t.Error(err)
		}
	}
	if _, _, err := s.SaveTechnical(t.Context(), w.ID, w.Site, w.Revision, "desktop", "structured-data", source.report.Report.FetchTime); !errors.Is(err, ErrConflict) {
		t.Fatalf("concurrent site change %v", err)
	}
}
func TestLegacyGSCFileReopensAndTechnicalKindDoesNotCollide(t *testing.T) {
	s, registry, w, source, path := technicalService(t)
	saveFinding(t, s, w)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(raw), "checkScore") || strings.Contains(string(raw), "verification") {
		t.Fatal("legacy DTO changed")
	}
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	s = New(repo, registry, source, nil)
	saveTechnical(t, s, w, source)
	value, _ := s.List(w.ID, w.Site)
	if len(value.Tasks) != 2 {
		t.Fatalf("mixed tasks %d", len(value.Tasks))
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, value.Tasks[0].ID, 1); !errors.Is(err, ErrInvalid) {
		t.Fatalf("GSC recheck accepted %v", err)
	}
}

func TestTechnicalReadOnlyPrerequisitesAndAllCheckStatuses(t *testing.T) {
	s, _, w, source, _ := technicalService(t)
	for _, status := range []string{"failed", "manual", "error"} {
		copy := *source.report.Report
		copy.Checks = []pulse.PageSpeedCheck{{ID: "structured-data", Status: status}}
		if status == "failed" {
			score := 0.0
			copy.Checks[0].Score = &score
		}
		source.report.Report = &copy
		if _, _, err := s.SaveTechnical(t.Context(), w.ID, w.Site, w.Revision, "desktop", "structured-data", copy.FetchTime); err != nil {
			t.Fatalf("actionable %s: %v", status, err)
		}
	}
	value, _ := s.List(w.ID, w.Site)
	task := value.Tasks[0]
	for index, status := range []string{"failed", "manual", "error", "not_applicable", "informative", "not_reported", "passed"} {
		source.report.Report.FetchTime = "2026-10-22T11:0" + string(rune('0'+index)) + ":00Z"
		score := 0.0
		var ptr *float64
		if status == "failed" {
			ptr = &score
		}
		if status == "passed" {
			score = 1
			ptr = &score
		}
		source.report.Report.Checks = []pulse.PageSpeedCheck{{ID: "structured-data", Status: status, Score: ptr}}
		value, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, int64(index+1))
		if err != nil || value.Tasks[0].Verification.Status != status {
			t.Fatalf("check %s: %+v %v", status, value, err)
		}
	}
	searchOnly, _, other, _, _ := testService(t)
	if _, _, err := searchOnly.SaveTechnical(t.Context(), other.ID, other.Site, other.Revision, "desktop", "structured-data", "2026-10-22T10:00:00Z"); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatalf("missing source %v", err)
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, "task_bad", 1); !errors.Is(err, ErrInvalid) {
		t.Fatal("invalid ID accepted")
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision+1, task.ID, 8); !errors.Is(err, ErrConflict) {
		t.Fatal("website revision accepted")
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, "task_00000000000000000000000000000000", 1); !errors.Is(err, ErrNotFound) {
		t.Fatal("missing task accepted")
	}
}

func TestTechnicalStateRejectsCrossVariantAndInvalidVerificationJSON(t *testing.T) {
	s, _, w, source, path := technicalService(t)
	value := saveTechnical(t, s, w, source)
	source.report.Report.FetchTime = "2026-10-22T11:00:00Z"
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, value.Tasks[0].ID, 1); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, bad := range []string{
		strings.Replace(string(raw), `"checkScore":null`, `"checkScore":null,"clicks":0`, 1),
		strings.Replace(string(raw), `"checkScore":null,`, ``, 1),
		strings.Replace(string(raw), `"checkStatus":"manual"`, `"checkStatus":"passed"`, 1),
		strings.Replace(string(raw), `"status":"manual","score":null`, `"status":"unknown","score":null`, 1),
		strings.Replace(string(raw), `"kind":"created"`, `"kind":"verified"`, 1),
		strings.Replace(string(raw), `"kind":"verified"`, `"kind":"created"`, 1),
	} {
		if err := os.WriteFile(path, []byte(bad), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(path, "org-one"); err == nil {
			t.Fatalf("corrupt variant accepted %s", bad)
		}
	}
	var parsed Evidence
	for _, raw := range []string{`{"kind":1}`, `{"kind":"pagespeed_check","key":"structured-data","unknown":1}`, `{"kind":"gsc_opportunity","strategy":"mobile"}`, `{} {}`} {
		if json.Unmarshal([]byte(raw), &parsed) == nil {
			t.Fatalf("invalid evidence decoded %s", raw)
		}
	}
}

func TestTechnicalVerificationRejectsMissingChangedAndMalformedReports(t *testing.T) {
	for _, change := range []func(*technicalStub){
		func(source *technicalStub) { source.report.Report = nil },
		func(source *technicalStub) { source.report.Report.Checks = nil },
		func(source *technicalStub) { source.report.Report.FetchTime = "bad-time" },
		func(source *technicalStub) { source.report.Report.FetchTime = "2026-10-22T11:00:00.1234567890Z" },
		func(source *technicalStub) { source.report.Report.FetchTime = "2026-10-22T11:00:00+24:00" },
		func(source *technicalStub) { source.report.Report.LighthouseVersion = "" },
		func(source *technicalStub) { source.report.Report.LighthouseVersion = "13.0 beta" },
		func(source *technicalStub) {
			source.report.Report.Checks = append(source.report.Report.Checks, source.report.Report.Checks[0])
		},
	} {
		s, _, w, source, _ := technicalService(t)
		value := saveTechnical(t, s, w, source)
		change(source)
		if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, value.Tasks[0].ID, 1); err == nil {
			t.Fatal("invalid report accepted")
		}
		got, _ := s.List(w.ID, w.Site)
		if got.Tasks[0].Verification != nil || len(got.Activity) != 1 {
			t.Fatal("invalid report mutated task")
		}
	}
	s, registry, w, source, _ := technicalService(t)
	value := saveTechnical(t, s, w, source)
	source.report.Report.FetchTime = "2026-10-22T11:00:00Z"
	source.during = func() {
		_, err := registry.Change(t.Context(), w.ID, "https://agenci.no/", w.Revision)
		if err != nil {
			t.Error(err)
		}
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, value.Tasks[0].ID, 1); !errors.Is(err, ErrConflict) {
		t.Fatalf("binding changed during verification %v", err)
	}
}

func TestTechnicalFutureObservationCannotPoisonStoredQueue(t *testing.T) {
	s, _, w, source, _ := technicalService(t)
	source.report.Report.FetchTime = "2026-10-22T13:00:00Z"
	if _, _, err := s.SaveTechnical(t.Context(), w.ID, w.Site, w.Revision, "desktop", "structured-data", source.report.Report.FetchTime); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatalf("future source save %v", err)
	}
	value, _ := s.List(w.ID, w.Site)
	if len(value.Tasks) != 0 {
		t.Fatal("future evidence stored")
	}
	source.report.Report.FetchTime = "2026-10-22T10:00:00Z"
	value = saveTechnical(t, s, w, source)
	source.report.Report.FetchTime = "2026-10-22T13:00:00Z"
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, value.Tasks[0].ID, 1); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatalf("future source verification %v", err)
	}
	value, _ = s.List(w.ID, w.Site)
	if value.Tasks[0].Verification != nil || value.Tasks[0].Revision != 1 || len(value.Activity) != 1 {
		t.Fatal("future verification stored")
	}
}
