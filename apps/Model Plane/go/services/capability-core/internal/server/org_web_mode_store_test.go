package server

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
)

// fakeOrgWebDB stands in for org_agent_policy. doc is the stored settings map; a key whose value is nil is a JSON
// null, which has no text. noRow is an org with no row at all.
type fakeOrgWebDB struct {
	doc     map[string]any
	noRow   bool
	err     error
	queries int
}

func (db *fakeOrgWebDB) QueryRow(_ context.Context, _ string, _ ...any) pgx.Row {
	db.queries++
	if db.err != nil {
		return orgWebScan{err: db.err}
	}
	if db.noRow {
		return orgWebScan{err: pgx.ErrNoRows}
	}
	// present reports whether key is in the document, and its text: a string for web.mode, and for web.covers_browser
	// only a JSON boolean (O-19), so a string "false" has no text. A null has no text.
	present := func(key string) (bool, *string) {
		value, ok := db.doc[key]
		if !ok {
			return false, nil
		}
		if key == "web.covers_browser" {
			flag, isBool := value.(bool)
			if !isBool {
				return true, nil
			}
			text := "false"
			if flag {
				text = "true"
			}
			return true, &text
		}
		text, isText := value.(string)
		if !isText {
			return true, nil
		}
		return true, &text
	}
	hasMode, mode := present("web.mode")
	hasCovers, covers := present("web.covers_browser")
	return orgWebScan{values: []any{hasMode, mode, hasCovers, covers}}
}

// orgWebScan is the one row of the store's query, read through Scan in column order.
type orgWebScan struct {
	values []any
	err    error
}

func (r orgWebScan) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	for i, target := range dest {
		switch out := target.(type) {
		case *bool:
			*out = r.values[i].(bool)
		case **string:
			*out = r.values[i].(*string)
		}
	}
	return nil
}

// The keys are read as the schema reads them: an absent key takes its default, and a present key is read as
// written. A JSON null has no word, so web.mode reads as unknown and denies the web, and covers_browser covers.
func TestOrgWebModeStoreReadsTheKeysAsWritten(t *testing.T) {
	for _, tc := range []struct {
		name       string
		noRow      bool
		doc        map[string]any
		wantMode   string
		wantCovers bool
	}{
		{name: "no row reads as live and covered", noRow: true, wantMode: "live", wantCovers: true},
		{name: "an empty document reads as live and covered", doc: map[string]any{}, wantMode: "live", wantCovers: true},
		{name: "off with the browser opted out", doc: map[string]any{"web.mode": "off", "web.covers_browser": false}, wantMode: "off", wantCovers: false},
		{name: "off with the browser covered", doc: map[string]any{"web.mode": "off", "web.covers_browser": true}, wantMode: "off", wantCovers: true},
		{name: "cached_only", doc: map[string]any{"web.mode": "cached_only"}, wantMode: "cached_only", wantCovers: true},
		{name: "a value this build does not know", doc: map[string]any{"web.mode": "full"}, wantMode: "full", wantCovers: true},
		{name: "a missing web.mode reads as live", doc: map[string]any{"web.covers_browser": false}, wantMode: "live", wantCovers: false},
		{name: "a JSON null web.mode has no word", doc: map[string]any{"web.mode": nil}, wantMode: "", wantCovers: true},
		{name: "a JSON null covers_browser is covered", doc: map[string]any{"web.covers_browser": nil}, wantMode: "live", wantCovers: true},
		{name: "a string false is not an opt-out", doc: map[string]any{"web.mode": "off", "web.covers_browser": "false"}, wantMode: "off", wantCovers: true},
		{name: "a string off is not an opt-out", doc: map[string]any{"web.mode": "off", "web.covers_browser": "off"}, wantMode: "off", wantCovers: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			store := newOrgWebModeStore(&fakeOrgWebDB{noRow: tc.noRow, doc: tc.doc}, time.Now)
			mode, covers, err := store.WebMode(context.Background(), "org-a")
			if err != nil {
				t.Fatalf("WebMode: %v", err)
			}
			if mode != tc.wantMode || covers != tc.wantCovers {
				t.Fatalf("WebMode = (%q, %v), want (%q, %v)", mode, covers, tc.wantMode, tc.wantCovers)
			}
		})
	}
}

// An org's answer is kept for five seconds, so a change in its policy shows once that window closes. Each org keeps
// its own answer.
func TestOrgWebModeStoreKeepsAnAnswerForFiveSeconds(t *testing.T) {
	start := time.Date(2026, 11, 24, 9, 0, 0, 0, time.UTC)
	now := start
	db := &fakeOrgWebDB{doc: map[string]any{"web.mode": "live"}}
	store := newOrgWebModeStore(db, func() time.Time { return now })
	ctx := context.Background()

	mode, _, err := store.WebMode(ctx, "org-a")
	if err != nil || mode != "live" || db.queries != 1 {
		t.Fatalf("first read = (%q, %v) after %d reads, want live after 1", mode, err, db.queries)
	}

	db.doc = map[string]any{"web.mode": "off"}
	now = start.Add(4 * time.Second)
	mode, _, err = store.WebMode(ctx, "org-a")
	if err != nil || mode != "live" || db.queries != 1 {
		t.Fatalf("a read inside the window = (%q, %v) after %d reads, want the kept live answer", mode, err, db.queries)
	}

	now = start.Add(5 * time.Second)
	mode, _, err = store.WebMode(ctx, "org-a")
	if err != nil || mode != "off" || db.queries != 2 {
		t.Fatalf("a read after the window = (%q, %v) after %d reads, want the change off", mode, err, db.queries)
	}

	db.doc = map[string]any{"web.mode": "cached_only"}
	mode, _, err = store.WebMode(ctx, "org-b")
	if err != nil || mode != "cached_only" || db.queries != 3 {
		t.Fatalf("org-b = (%q, %v) after %d reads, want its own read of cached_only", mode, err, db.queries)
	}
}

// An answer that has expired is dropped when the next org is written, so the map holds only the orgs read in the
// last window.
func TestOrgWebModeStoreDropsExpiredAnswers(t *testing.T) {
	start := time.Date(2026, 11, 24, 9, 0, 0, 0, time.UTC)
	now := start
	db := &fakeOrgWebDB{doc: map[string]any{"web.mode": "live"}}
	store := newOrgWebModeStore(db, func() time.Time { return now })
	ctx := context.Background()

	for _, org := range []string{"org-a", "org-b"} {
		if _, _, err := store.WebMode(ctx, org); err != nil {
			t.Fatalf("WebMode(%s): %v", org, err)
		}
	}
	if len(store.cache) != 2 {
		t.Fatalf("cached orgs = %d, want 2", len(store.cache))
	}
	now = start.Add(6 * time.Second)
	if _, _, err := store.WebMode(ctx, "org-c"); err != nil {
		t.Fatalf("WebMode(org-c): %v", err)
	}
	if len(store.cache) != 1 {
		t.Fatalf("cached orgs after the window = %d, want only org-c", len(store.cache))
	}
}

// A failed read is never kept: the next call asks the database again, and the answer it gets is the one in force.
func TestOrgWebModeStoreDoesNotKeepAFailedRead(t *testing.T) {
	db := &fakeOrgWebDB{err: errors.New("database unavailable")}
	store := newOrgWebModeStore(db, time.Now)
	ctx := context.Background()

	if _, _, err := store.WebMode(ctx, "org-a"); err == nil {
		t.Fatal("a failed read returned no error")
	}
	db.err = nil
	db.doc = map[string]any{"web.mode": "off"}
	mode, _, err := store.WebMode(ctx, "org-a")
	if err != nil || mode != "off" || db.queries != 2 {
		t.Fatalf("after a failed read = (%q, %v) after %d reads, want the database read again: off", mode, err, db.queries)
	}
}
