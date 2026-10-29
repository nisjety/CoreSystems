package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
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
	"github.com/triodelab/anbud/internal/sources/doffin"
)

const e2eKey = "internal-api-key-value"

func storeNotice(t *testing.T, store *rawstore.Store, run int64, id string, body []byte) string {
	t.Helper()
	sum := sha256.Sum256(body)
	n := doffin.Notice{ID: id, ContentType: "application/xml", Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: time.Now().UTC()}
	if _, err := store.StoreNotice(context.Background(), run, n); err != nil {
		t.Fatalf("StoreNotice %s: %v", id, err)
	}
	return n.SHA256
}

func call(t *testing.T, app interface {
	Test(*http.Request, ...int) (*http.Response, error)
}, path, body string) (int, map[string]any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
	req.Header.Set("X-API-Key", e2eKey)
	resp, err := app.Test(req, 20000)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	var out map[string]any
	if err := json.Unmarshal(raw, &out); err != nil {
		t.Fatalf("%s: not JSON: %q", path, raw)
	}
	return resp.StatusCode, out
}

// The demo as production wires it: a real database, the raw store, the service and the HTTP layer.
func TestDemoEndToEndAgainstARealDatabase(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx := context.Background()
	day := time.Date(2026, time.October, 1, 0, 0, 0, 0, time.UTC)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatalf("BeginRun: %v", err)
	}

	open := time.Now().UTC().Add(20 * 24 * time.Hour)
	hash := storeNotice(t, store, run, "2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "Åpen drift", CPV: "72212000", Deadline: open}))
	storeNotice(t, store, run, "2026-100002", eformstest.ContractNotice(eformstest.Notice{Title: "Slettes", CPV: "72212000", Deadline: open}))
	storeNotice(t, store, run, "2026-100003", eformstest.AwardNotice(eformstest.Notice{}))
	storeNotice(t, store, run, "2026-100004", []byte("not a notice"))

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
	if funnel["scanned"] != float64(2) || funnel["awards"] != float64(0) || funnel["unreadable"] != float64(0) || funnel["opportunities"] != float64(2) {
		t.Errorf("funnel = %v", funnel)
	}
	if results, _ := data["results"].([]any); len(results) != 2 {
		t.Errorf("results = %v, want both open tenders", data["results"])
	}

	status, body = call(t, app, "/api/v1/demo/matches/2026-100001", profile)
	audit, _ := body["data"].(map[string]any)
	evidence, _ := audit["evidence"].(map[string]any)
	if status != 200 || evidence["sha256"] != hash || evidence["source"] != "doffin" {
		t.Errorf("audit: status %d evidence %v, want the stored hash %s", status, evidence, hash)
	}

	// Erase one body the way a DBA does (docs/PERSONAL-DATA.md). It must not be read again.
	var rawID int64
	if err := pool.QueryRow(ctx, `SELECT id FROM raw_documents WHERE doffin_id = '2026-100002'`).Scan(&rawID); err != nil {
		t.Fatalf("find the row: %v", err)
	}
	if _, err := pool.Exec(ctx, `SELECT anbud_redact_raw_document($1, 'erasure_request', 'TEST-1')`, rawID); err != nil {
		t.Fatalf("redact: %v", err)
	}

	_, body = call(t, app, "/api/v1/demo/matches", profile)
	data, _ = body["data"].(map[string]any)
	results, _ := data["results"].([]any)
	if len(results) != 1 {
		t.Fatalf("after the erasure: results = %v, want the one tender left", data["results"])
	}
	if first, _ := results[0].(map[string]any); first["notice_id"] != "2026-100001" {
		t.Errorf("the erased notice is still listed: %v", first)
	}
	if status, body = call(t, app, "/api/v1/demo/matches/2026-100002", profile); status != 404 {
		t.Errorf("audit of the erased notice: status %d %v, want 404", status, body)
	}
}
