package main

import (
	"context"
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

// An amended tender as the demo serves it, as production wires it: a real database, the raw store, the
// service and the HTTP layer. A change notice of a tender is stored next to the tender; the list shows
// the change alone, and the audit sheet of the tender shows both. Erasing the change body (docs/PERSONAL-
// DATA.md) takes it out of both at once, and the tender is the one it was before. Needs a throwaway
// server: run with make test-db.
func TestAmendmentEndToEndAgainstARealDatabase(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx := context.Background()
	day := time.Date(2026, time.October, 1, 0, 0, 0, 0, time.UTC)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatalf("BeginRun: %v", err)
	}

	// Dates are relative to today, so the test stays true.
	today := time.Now().UTC().Truncate(24 * time.Hour)
	const procedure = "00000000-0000-4000-8000-0000000000d1"
	const originalUUID = "00000000-0000-4000-8000-00000000d001"
	const changeUUID = "00000000-0000-4000-8000-00000000d002"
	storeNotice(t, store, run, "2026-100011", eformstest.ContractNotice(eformstest.Notice{
		ID: originalUUID, ProcedureID: procedure, Title: "Drift av fagsystem", CPV: "72212000",
		Deadline: today.Add(20 * 24 * time.Hour), IssuedAt: today.AddDate(0, 0, -14),
	}))
	storeNotice(t, store, run, "2026-100012", eformstest.ContractNotice(eformstest.Notice{
		ID: changeUUID, ProcedureID: procedure, Title: "Drift av fagsystem", CPV: "72212000",
		Deadline: today.Add(34 * 24 * time.Hour), IssuedAt: today.AddDate(0, 0, -7),
		Change: &eformstest.Change{Replaces: originalUUID + "-01", Reason: "Fristen er utsatt.", Sections: []string{"LOT-0001"}},
	}))
	storeNotice(t, store, run, "2026-100013", eformstest.ContractNotice(eformstest.Notice{
		ID: "00000000-0000-4000-8000-00000000d003", ProcedureID: "00000000-0000-4000-8000-0000000000d2",
		Title: "Annen drift", CPV: "72212000", Deadline: today.Add(10 * 24 * time.Hour), IssuedAt: today.AddDate(0, 0, -3),
	}))

	svc, err := buildDemo(config.Config{}, store, nil, zerolog.Nop())
	if err != nil {
		t.Fatalf("buildDemo: %v", err)
	}
	if err := svc.PrepareCatalogue(ctx); err != nil {
		t.Fatal(err)
	}
	app := api.NewServer(api.ServerConfig{APIKey: secret.Value(e2eKey), Demo: svc})
	const profile = `{"profile":{"cpv":["72000000"]}}`

	status, body := call(t, app, "/api/v1/demo/matches", profile)
	data, _ := body["data"].(map[string]any)
	if status != 200 || data == nil {
		t.Fatalf("matches: status %d %v", status, body)
	}
	funnel, _ := data["funnel"].(map[string]any)
	if funnel["scanned"] != float64(3) || funnel["superseded"] != float64(1) || funnel["opportunities"] != float64(2) {
		t.Errorf("funnel = %v, want 3 scanned, 1 superseded, 2 current", funnel)
	}
	results, _ := data["results"].([]any)
	if len(results) != 2 {
		t.Fatalf("results = %v, want the change and the other tender", data["results"])
	}
	for _, r := range results {
		row, _ := r.(map[string]any)
		if row["notice_id"] == "2026-100011" {
			t.Errorf("the superseded tender is listed: %v", row)
		}
		if row["notice_id"] == "2026-100012" {
			am, _ := row["amendment"].(map[string]any)
			if am == nil || am["versions"] != float64(2) || am["confirmed"] != true || am["reason"] != "Fristen er utsatt." {
				t.Errorf("amendment of the change = %v", am)
			}
		}
	}

	status, body = call(t, app, "/api/v1/demo/matches/2026-100011", profile)
	audit, _ := body["data"].(map[string]any)
	if status != 200 || audit == nil {
		t.Fatalf("audit of the original: status %d %v", status, body)
	}
	am, _ := audit["amendments"].(map[string]any)
	if am == nil || am["position"] != float64(1) || am["superseded_by"] != "2026-100012" {
		t.Errorf("amendments of the original = %v, want position 1, superseded by the change", am)
	}

	// Erase the change body the way a DBA does. It must leave the list and the history at once.
	var rawID int64
	if err := pool.QueryRow(ctx, `SELECT id FROM raw_documents WHERE doffin_id = '2026-100012'`).Scan(&rawID); err != nil {
		t.Fatalf("find the row: %v", err)
	}
	if _, err := pool.Exec(ctx, `SELECT anbud_redact_raw_document($1, 'erasure_request', 'TEST-3')`, rawID); err != nil {
		t.Fatalf("redact: %v", err)
	}
	_, body = call(t, app, "/api/v1/demo/matches", profile)
	data, _ = body["data"].(map[string]any)
	funnel, _ = data["funnel"].(map[string]any)
	if funnel["superseded"] != float64(0) || funnel["scanned"] != float64(2) {
		t.Errorf("after the erasure: funnel = %v, want the change gone and nothing superseded", funnel)
	}
	results, _ = data["results"].([]any)
	for _, r := range results {
		row, _ := r.(map[string]any)
		if row["notice_id"] == "2026-100012" || row["amendment"] != nil {
			t.Errorf("after the erasure the erased change or its amendment is still listed: %v", row)
		}
	}
	_, body = call(t, app, "/api/v1/demo/matches/2026-100011", profile)
	audit, _ = body["data"].(map[string]any)
	if audit["amendments"] != nil {
		t.Errorf("after the erasure the original still has a history: %v", audit["amendments"])
	}
}
