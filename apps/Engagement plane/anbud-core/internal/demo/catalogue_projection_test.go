package demo

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"github.com/triodelab/anbud/internal/workspace"
)

func TestPreparedCatalogueParityRefreshClockAndRedaction(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx := context.Background()
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: now, To: now})
	if err != nil {
		t.Fatal(err)
	}
	var docs []rawstore.Document
	add := func(id, title string) {
		t.Helper()
		body := eformstest.ContractNotice(eformstest.Notice{Title: title, CPV: "72000000", Deadline: in(2)})
		sum := sha256.Sum256(body)
		hash := hex.EncodeToString(sum[:])
		if _, err := store.StoreNotice(ctx, run, doffin.Notice{ID: id, Body: body, SHA256: hash, FetchedAt: now}); err != nil {
			t.Fatal(err)
		}
		docs = append(docs, rawstore.Document{Source: "doffin", ID: id, Body: body, SHA256: hash, FetchedAt: now})
	}
	add("2026-100001", "Rammeavtale – IT")
	add("2026-100002", "Renhold")
	s := newService(t, store, nil)
	if err := s.PrepareCatalogue(ctx); err != nil {
		t.Fatal(err)
	}
	oracle := newService(t, &pagedNotices{fakeNotices{docs: docs}}, nil)
	for _, request := range []CatalogueRequest{{}, {Query: "rammeavtale - IT"}, {Status: "active"}, {Sort: "title", Direction: "desc"}, {CPV: "72", Region: "NO08"}} {
		actual, err := s.Catalogue(ctx, request)
		if err != nil {
			t.Fatal(err)
		}
		want, err := oracle.Catalogue(ctx, request)
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(actual.Results, want.Results) || !reflect.DeepEqual(actual.Facets, want.Facets) {
			a, _ := json.Marshal(actual)
			w, _ := json.Marshal(want)
			t.Fatalf("projection parity mismatch: %s vs %s", a, w)
		}
	}
	work := workspace.New(pool)
	if _, err := work.SetStage(ctx, "org-a", "user-a", "2026-100001", "working"); err != nil {
		t.Fatal(err)
	}
	s.tracking = work
	kept, err := s.Matches(ctx, Request{Org: "org-a", TrackedOnly: true})
	if err != nil || len(kept.TrackedResults) != 1 {
		t.Fatalf("saved pipeline without a profile: %+v %v", kept, err)
	}
	isolated, err := s.Matches(ctx, Request{Org: "org-b", TrackedOnly: true})
	if err != nil || len(isolated.TrackedResults) != 0 {
		t.Fatalf("pipeline crossed organisations: %+v %v", isolated, err)
	}
	pipeline, err := s.Matches(ctx, Request{Profile: match.Profile{CPV: []string{"72000000"}}, Limit: 1})
	if err != nil || pipeline.ActiveAssessed != 2 || len(pipeline.Results) != 1 {
		t.Fatalf("prepared active matching incomplete: %+v %v", pipeline, err)
	}
	generation, err := store.CatalogueGeneration(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := s.PrepareCatalogue(ctx); err != nil {
		t.Fatal(err)
	}
	unchanged, err := store.CatalogueGeneration(ctx)
	if err != nil || generation != unchanged {
		t.Fatalf("unchanged preparation writes: %d -> %d (%v)", generation, unchanged, err)
	}
	clock := now.Add(4 * 24 * time.Hour)
	s.now = func() time.Time { return clock }
	expired, err := s.Catalogue(ctx, CatalogueRequest{Status: "expired"})
	if err != nil || len(expired.Results) != 2 {
		t.Fatalf("cached deadlines did not expire: %+v %v", expired, err)
	}
	closed, err := s.Matches(ctx, Request{Profile: match.Profile{CPV: []string{"72000000"}}})
	if err != nil || len(closed.Results) != 0 || closed.ActiveAssessed != 0 {
		t.Fatalf("expired prepared tenders still matched: %+v %v", closed, err)
	}
	s.now = func() time.Time { return now }
	add("2026-100003", "Ny anskaffelse")
	if err := s.PrepareCatalogue(ctx); err != nil {
		t.Fatal(err)
	}
	got, err := s.Catalogue(ctx, CatalogueRequest{})
	if err != nil || len(got.Results) != 3 {
		t.Fatalf("import did not refresh: %+v %v", got, err)
	}
	var rawID int64
	if err := pool.QueryRow(ctx, `SELECT id FROM raw_documents WHERE doffin_id='2026-100001'`).Scan(&rawID); err != nil {
		t.Fatal(err)
	}
	if _, err := pool.Exec(ctx, `SELECT anbud_redact_raw_document($1,'erasure_request','TEST-CATALOGUE')`, rawID); err != nil {
		t.Fatal(err)
	}
	got, err = s.Catalogue(ctx, CatalogueRequest{})
	if err != nil || len(got.Results) != 2 {
		t.Fatalf("erasure not visible immediately: %+v %v", got, err)
	}
	var held int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM catalogue_projection WHERE raw_document_id=$1`, rawID).Scan(&held); err != nil || held != 0 {
		t.Fatalf("derived data survived erasure: %d %v", held, err)
	}
	if err := store.SaveCatalogueProjection(ctx, catalogueProjectionVersion, []rawstore.CatalogueProjection{{Sequence: rawID, Payload: json.RawMessage(`{"notice":{}}`)}}); err != nil {
		t.Fatal(err)
	}
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM catalogue_projection WHERE raw_document_id=$1`, rawID).Scan(&held); err != nil || held != 0 {
		t.Fatalf("stale preparer resurrected erasure: %d %v", held, err)
	}
}
