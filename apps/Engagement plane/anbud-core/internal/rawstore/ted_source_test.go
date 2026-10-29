package rawstore

import (
	"errors"
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/noticeid"
)

// A second source (migration 0008). A Doffin notice and a TED notice whose identifiers are the same
// digits are two notices: each is stored under its own source, the readers give each its public id,
// and a read by public id finds the right body. Each source has its own day ledger.
func TestTEDAndDoffinNoticesWithTheSameDigitsStayApart(t *testing.T) {
	doffinStore, pool := openStore(t)
	tedStore, err := NewForSource(pool, noticeid.SourceTED)
	if err != nil {
		t.Fatalf("NewForSource: %v", err)
	}
	ctx := testCtx(t)
	run := beginSyncRun(t, doffinStore, "2026-10-01", "2026-10-01")

	if _, err := doffinStore.StoreNotice(ctx, run, guardNotice("1234-2026", []byte("<doffin/>"))); err != nil {
		t.Fatalf("store the Doffin notice: %v", err)
	}
	if _, err := tedStore.StoreNotice(ctx, run, guardNotice("1234-2026", []byte("<ted/>"))); err != nil {
		t.Fatalf("store the TED notice: %v", err)
	}

	for public, body := range map[string]string{"1234-2026": "<doffin/>", "TED-1234-2026": "<ted/>"} {
		doc, err := doffinStore.Notice(ctx, public)
		if err != nil || string(doc.Body) != body || doc.ID != public {
			t.Errorf("Notice(%s) = %q %q, %v; want %q", public, doc.ID, doc.Body, err, body)
		}
		held, err := tedStore.HasNotice(ctx, public)
		if err != nil || !held {
			t.Errorf("HasNotice(%s) = %v, %v; want held", public, held, err)
		}
	}

	var ids []string
	if err := doffinStore.RecentNotices(ctx, 10, func(d Document) error {
		ids = append(ids, d.Source+" "+d.ID)
		return nil
	}); err != nil {
		t.Fatalf("RecentNotices: %v", err)
	}
	if len(ids) != 2 || ids[0] != "ted TED-1234-2026" || ids[1] != "doffin 1234-2026" {
		t.Errorf("recent = %v, want both, newest first, under their public ids", ids)
	}
	page, err := doffinStore.CataloguePage(ctx, 0, 10)
	if err != nil || len(page) != 2 || page[0].Document.ID != "TED-1234-2026" || page[1].Document.ID != "1234-2026" {
		t.Errorf("catalogue = %+v, %v; want both under their public ids", page, err)
	}
	if total, err := doffinStore.CatalogueTotal(ctx); err != nil || total != 2 {
		t.Errorf("catalogue total = %d, %v; want 2", total, err)
	}

	// Resolution is the store's own source: the TED sync sees its notice as held, and the Doffin
	// sync sees its own.
	for _, s := range []*Store{doffinStore, tedStore} {
		res, err := s.Resolve(ctx, []string{"1234-2026"})
		if err != nil || res["1234-2026"].Kind != doffinsync.ResolutionHeld {
			t.Errorf("%s Resolve = %+v, %v; want held", s.Source(), res, err)
		}
	}

	// The day ledger is per source.
	day := utcDay(t, "2026-10-01")
	if err := tedStore.BeginDay(ctx, day, run); err != nil {
		t.Fatalf("TED BeginDay: %v", err)
	}
	states, err := doffinStore.DayStates(ctx, day, day)
	if err != nil || len(states) != 0 {
		t.Errorf("Doffin day states = %v, %v; want none: the TED day is TED's", states, err)
	}
	if states, err := tedStore.DayStates(ctx, day, day); err != nil || len(states) != 1 {
		t.Errorf("TED day states = %v, %v; want the TED day", states, err)
	}
}

// Each store refuses an identifier of the other source's shape before any SQL, and the database
// refuses it too when the store's check is out of the way.
func TestASourceTakesOnlyItsOwnIdentifierShape(t *testing.T) {
	doffinStore, pool := openStore(t)
	tedStore, _ := NewForSource(pool, noticeid.SourceTED)
	ctx := testCtx(t)
	run := beginSyncRun(t, doffinStore, "2026-10-01", "2026-10-01")

	if _, err := tedStore.StoreNotice(ctx, run, guardNotice("2026-116564", []byte("<x/>"))); !errors.Is(err, ErrInvalidNotice) {
		t.Errorf("TED store took a Doffin id: %v", err)
	}
	if _, err := doffinStore.StoreNotice(ctx, run, guardNotice("741144-2026", []byte("<x/>"))); !errors.Is(err, ErrInvalidNotice) {
		t.Errorf("Doffin store took a TED number: %v", err)
	}
	for _, row := range []struct{ source, id string }{
		{"ted", "2026-116564"},
		{"doffin", "741144-2026"},
		{"other", "2026-116564"},
	} {
		_, err := pool.Exec(ctx, `INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id)
VALUES ($1, $2, '', '\x3c782f3e', encode(sha256('\x3c782f3e'::bytea), 'hex'), now(), $3)`, row.source, row.id, run)
		if err == nil {
			t.Errorf("the database took source %s with id %s", row.source, row.id)
		}
	}
	if _, err := NewForSource(pool, "other"); !errors.Is(err, ErrUnknownSource) {
		t.Errorf("NewForSource(other) = %v, want ErrUnknownSource", err)
	}
}
