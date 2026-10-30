package main

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/api"
	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/secret"
)

// The incumbency panel and the re-tender radar as production wires them: a real database, the
// raw store, the service and the HTTP layer. Needs a throwaway server: run with make test-db.
//
// Two awards of one buyer are stored next to the tender being audited. One body is then erased the
// way a DBA erases it (docs/PERSONAL-DATA.md). The erased award must leave the panel and the radar
// at once, because the demo reads raw_documents_live and keeps nothing. No register is configured,
// so no winner may be named in any response.
func TestIncumbencyEndToEndAgainstARealDatabase(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx := context.Background()
	day := time.Date(2026, time.October, 1, 0, 0, 0, 0, time.UTC)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatalf("BeginRun: %v", err)
	}

	// Dates are relative to today so the test stays true: both contracts run 24 months from an
	// award made 20 months ago, so each ends in about four months.
	today := time.Now().UTC().Truncate(24 * time.Hour)
	awarded := today.AddDate(0, -20, 0)
	open := today.Add(20 * 24 * time.Hour)

	const (
		winnerKept   = "Fiktiv Leverandør AS"
		winnerErased = "Annen Fiktiv AS"
	)
	storeNotice(t, store, run, "2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "Drift av fagsystem", CPV: "72212000", Deadline: open}))
	storeNotice(t, store, run, "2025-200001", eformstest.AwardWithResult(
		eformstest.Notice{ID: "00000000-0000-4000-8000-000000000a01", ProcedureID: "00000000-0000-4000-8000-0000000000a1", CPV: "72212000"},
		eformstest.Award{WinnerName: winnerKept, WinnerOrg: "111111111", Tenders: 4, AwardDate: awarded, DurationMonths: 24}))
	storeNotice(t, store, run, "2025-200002", eformstest.AwardWithResult(
		eformstest.Notice{ID: "00000000-0000-4000-8000-000000000a02", ProcedureID: "00000000-0000-4000-8000-0000000000a2", CPV: "72212000"},
		eformstest.Award{WinnerName: winnerErased, WinnerOrg: "222222222", Tenders: 2, AwardDate: awarded, DurationMonths: 24}))

	svc, err := buildDemo(config.Config{}, store, nil, zerolog.Nop())
	if err != nil {
		t.Fatalf("buildDemo: %v", err)
	}
	app := api.NewServer(api.ServerConfig{APIKey: secret.Value(e2eKey), Demo: svc})

	const profile = `{"profile":{"cpv":["72000000"]}}`
	panelOf := func() (map[string]any, string) {
		t.Helper()
		status, body := call(t, app, "/api/v1/demo/matches/2026-100001", profile)
		raw, _ := json.Marshal(body)
		audit, _ := body["data"].(map[string]any)
		if status != 200 || audit == nil {
			t.Fatalf("audit: status %d %s", status, raw)
		}
		return audit, string(raw)
	}
	radarOf := func() ([]any, string) {
		t.Helper()
		status, body := call(t, app, "/api/v1/demo/retenders", `{"profile":{"cpv":["72000000"]},"horizon_months":12}`)
		raw, _ := json.Marshal(body)
		data, _ := body["data"].(map[string]any)
		if status != 200 || data == nil {
			t.Fatalf("retenders: status %d %s", status, raw)
		}
		results, _ := data["results"].([]any)
		return results, string(raw)
	}

	// Before the erasure: both awards are the buyer's history, and both contracts are on the radar.
	audit, raw := panelOf()
	panel, _ := audit["incumbency"].(map[string]any)
	status, _ := audit["incumbency_status"].(map[string]any)
	if panel == nil || status == nil || status["checked"] != true || status["awards"] != float64(2) || status["winners"] != "not_configured" {
		t.Fatalf("incumbency = %v status = %v", panel, status)
	}
	if panel["related"] != float64(2) || panel["median_tenders_received"] != float64(3) {
		t.Errorf("related %v median tenders %v, want 2 and 3 (of 4 and 2)", panel["related"], panel["median_tenders_received"])
	}
	incumbent, _ := panel["incumbent"].(map[string]any)
	if incumbent == nil || incumbent["relation"] != "same_cpv" || incumbent["end_basis"] != "awarded+duration" || incumbent["contract_end"] == "" {
		t.Errorf("incumbent = %v", incumbent)
	}
	if results, _ := radarOf(); len(results) != 2 {
		t.Errorf("radar before the erasure = %d entries, want 2", len(results))
	}
	for _, name := range []string{winnerKept, winnerErased, "111111111", "222222222"} {
		if strings.Contains(raw, name) {
			t.Errorf("without a register the audit sheet names a winner: %q", name)
		}
	}
	for _, marker := range eformstest.PersonalMarkers {
		if strings.Contains(raw, marker) {
			t.Errorf("the audit sheet carries %q", marker)
		}
	}

	// Erase one award's body the way a DBA does. It must not be read again.
	var rawID int64
	if err := pool.QueryRow(ctx, `SELECT id FROM raw_documents WHERE doffin_id = '2025-200002'`).Scan(&rawID); err != nil {
		t.Fatalf("find the row: %v", err)
	}
	if _, err := pool.Exec(ctx, `SELECT anbud_redact_raw_document($1, 'erasure_request', 'TEST-2')`, rawID); err != nil {
		t.Fatalf("redact: %v", err)
	}

	audit, raw = panelOf()
	panel, _ = audit["incumbency"].(map[string]any)
	status, _ = audit["incumbency_status"].(map[string]any)
	if panel == nil || panel["related"] != float64(1) || status["awards"] != float64(1) {
		t.Fatalf("after the erasure: panel %v status %v, want one related award", panel, status)
	}
	awards, _ := panel["awards"].([]any)
	if len(awards) != 1 {
		t.Fatalf("after the erasure the panel lists %d awards, want 1", len(awards))
	}
	if first, _ := awards[0].(map[string]any); first["notice_id"] != "2025-200001" || first["tenders_received"] != float64(4) {
		t.Errorf("the erased award is still on the panel, or the wrong one is: %v", first)
	}
	if panel["median_tenders_received"] != float64(4) {
		t.Errorf("median tenders = %v, want 4: the erased award's count still counts", panel["median_tenders_received"])
	}
	if strings.Contains(raw, "2025-200002") {
		t.Errorf("the erased award's notice id is still in the audit sheet")
	}
	results, raw := radarOf()
	if len(results) != 1 {
		t.Errorf("radar after the erasure = %d entries, want 1", len(results))
	}
	if strings.Contains(raw, "2025-200002") {
		t.Errorf("the erased award is still on the radar")
	}
}
