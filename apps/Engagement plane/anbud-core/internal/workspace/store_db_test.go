package workspace

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Needs a throwaway server on ANBUD_TEST_DSN: run it with make test-db. The store runs as anbud_app, the
// role the API runs as, so the row level policy is what keeps two organisations apart.
func TestOneOrganisationsWorkspaceIsInvisibleToAnother(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	if err := db.ApplyMigrations(context.Background(), dbtest.Pool(t, rs.MigratorDSN)); err != nil {
		t.Fatalf("migrate: %s", dbtest.Describe(err))
	}
	s := New(dbtest.Pool(t, rs.AppDSN))
	ctx := context.Background()
	by := "user-1"

	profile := json.RawMessage(`{"cpv":["72212000"],"nuts":["NO08"],"keywords":["fagsystem"]}`)
	if _, err := s.PutProfile(ctx, "org-a", by, profile); err != nil {
		t.Fatalf("save profile: %v", err)
	}
	got, err := s.GetProfile(ctx, "org-a")
	if err != nil || string(got.Profile) == "" {
		t.Fatalf("read profile: %v", err)
	}
	if _, err := s.GetProfile(ctx, "org-b"); !errors.Is(err, ErrNotFound) {
		t.Errorf("org-b reads org-a's profile: err %v, want ErrNotFound", err)
	}

	if _, err := s.SetStage(ctx, "org-a", by, "2026-116131", "working"); err != nil {
		t.Fatalf("set stage: %v", err)
	}
	if _, err := s.SetStage(ctx, "org-a", by, "2026-116131", "sent"); err != nil {
		t.Fatalf("change stage: %v", err)
	}
	items, err := s.ListPipeline(ctx, "org-a")
	if err != nil || len(items) != 1 || items[0].Stage != "sent" {
		t.Fatalf("pipeline of org-a = %+v, %v; want one item, sent", items, err)
	}
	if other, _ := s.ListPipeline(ctx, "org-b"); len(other) != 0 {
		t.Errorf("org-b sees org-a's pipeline: %+v", other)
	}

	saved, err := s.SaveSearch(ctx, "org-a", by, "Drift", json.RawMessage(`{"query":"drift","region":"NO08"}`))
	if err != nil {
		t.Fatalf("save search: %v", err)
	}
	if list, _ := s.ListSearches(ctx, "org-b"); len(list) != 0 {
		t.Errorf("org-b sees org-a's searches: %+v", list)
	}
	if err := s.DeleteSearch(ctx, "org-b", saved.ID); !errors.Is(err, ErrNotFound) {
		t.Errorf("org-b deletes org-a's search: err %v, want ErrNotFound", err)
	}
	if err := s.DeleteSearch(ctx, "org-a", saved.ID); err != nil {
		t.Errorf("org-a deletes its own search: %v", err)
	}
}

func TestSavedSearchesAreCapped(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	if err := db.ApplyMigrations(context.Background(), dbtest.Pool(t, rs.MigratorDSN)); err != nil {
		t.Fatalf("migrate: %s", dbtest.Describe(err))
	}
	s := New(dbtest.Pool(t, rs.AppDSN))
	ctx := context.Background()
	for i := 0; i < maxSearches; i++ {
		if _, err := s.SaveSearch(ctx, "org-a", "user-1", "Søk", json.RawMessage(`{}`)); err != nil {
			t.Fatalf("save %d: %v", i, err)
		}
	}
	if _, err := s.SaveSearch(ctx, "org-a", "user-1", "En til", json.RawMessage(`{}`)); !errors.Is(err, ErrTooMany) {
		t.Errorf("search %d: err %v, want ErrTooMany", maxSearches+1, err)
	}
}

// A suggestion creates a profile only where the organisation has none: once one is saved, a second
// suggestion is refused and the saved profile is kept.
func TestASuggestedProfileNeverReplacesOneTheOrganisationSaved(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	if err := db.ApplyMigrations(context.Background(), dbtest.Pool(t, rs.MigratorDSN)); err != nil {
		t.Fatalf("migrate: %s", dbtest.Describe(err))
	}
	s := New(dbtest.Pool(t, rs.AppDSN))
	ctx := context.Background()
	const org = "org-suggest"

	first := json.RawMessage(`{"cpv":["72212000"]}`)
	if _, created, err := s.PutProfileIfAbsent(ctx, org, "user-1", first); err != nil || !created {
		t.Fatalf("the first suggestion: created=%v err=%v", created, err)
	}
	if _, created, err := s.PutProfileIfAbsent(ctx, org, "user-1", json.RawMessage(`{"cpv":["45000000"]}`)); err != nil || created {
		t.Fatalf("a second suggestion: created=%v err=%v, want refused and no error", created, err)
	}
	saved, err := s.GetProfile(ctx, org)
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if !strings.Contains(string(saved.Profile), "72212000") || strings.Contains(string(saved.Profile), "45000000") {
		t.Errorf("the saved profile was replaced: %s", saved.Profile)
	}
}

// When an organisation marked its tenders as seen is its own: another organisation reads no mark of it.
func TestASeenMarkIsInvisibleToAnotherOrganisation(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	if err := db.ApplyMigrations(context.Background(), dbtest.Pool(t, rs.MigratorDSN)); err != nil {
		t.Fatalf("migrate: %s", dbtest.Describe(err))
	}
	s := New(dbtest.Pool(t, rs.AppDSN))
	ctx := context.Background()

	if _, ok, err := s.Seen(ctx, "org-seen-a"); err != nil || ok {
		t.Fatalf("before any mark: ok=%v err=%v", ok, err)
	}
	if _, err := s.MarkSeen(ctx, "org-seen-a", "user-1"); err != nil {
		t.Fatalf("mark: %v", err)
	}
	if _, ok, err := s.Seen(ctx, "org-seen-a"); err != nil || !ok {
		t.Errorf("the organisation's own mark: ok=%v err=%v", ok, err)
	}
	if _, ok, err := s.Seen(ctx, "org-seen-b"); err != nil || ok {
		t.Errorf("another organisation reads the mark: ok=%v err=%v", ok, err)
	}
}
