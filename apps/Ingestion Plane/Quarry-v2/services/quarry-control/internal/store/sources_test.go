package store

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func newSource(org, name, url string) Source {
	now := time.Now().UnixMilli()
	return Source{
		ID:        quarrycontracts.NewID(quarrycontracts.KindSource),
		OrgID:     org,
		Name:      name,
		URL:       url,
		Kind:      "scrape",
		Status:    "active",
		CreatedAt: now,
		UpdatedAt: now,
	}
}

func TestMemorySources_OrgScopedCRUD(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Sources()

	a1 := newSource("org_a", "A blog", "https://a.example/blog")
	a2 := newSource("org_a", "A pricing", "https://a.example/pricing")
	b1 := newSource("org_b", "B blog", "https://b.example/blog")
	for _, src := range []Source{a1, a2, b1} {
		if err := s.Create(src); err != nil {
			t.Fatalf("create %s: %v", src.ID, err)
		}
	}

	// ListByOrg returns only the caller's org rows.
	aList, _ := s.ListByOrg("org_a", 50, "")
	if len(aList) != 2 {
		t.Fatalf("org_a list len=%d want=2", len(aList))
	}
	bList, _ := s.ListByOrg("org_b", 50, "")
	if len(bList) != 1 {
		t.Fatalf("org_b list len=%d want=1", len(bList))
	}
	if bList[0].ID != b1.ID {
		t.Fatalf("org_b list returned %s want=%s", bList[0].ID, b1.ID)
	}

	// GetByOrg is org-guarded: org_b CANNOT read org_a's source by id.
	if _, ok := s.GetByOrg("org_b", a1.ID); ok {
		t.Fatal("IDOR: org_b read org_a's source via GetByOrg")
	}
	if got, ok := s.GetByOrg("org_a", a1.ID); !ok || got.ID != a1.ID {
		t.Fatalf("org_a could not read its own source: ok=%v", ok)
	}
}

func TestMemorySources_SoftDeleteIsOrgScopedAndHides(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Sources()

	a1 := newSource("org_a", "A blog", "https://a.example/blog")
	b1 := newSource("org_b", "B blog", "https://b.example/blog")
	_ = s.Create(a1)
	_ = s.Create(b1)

	// org_b attempting to delete org_a's source must NOT succeed — and must
	// leave org_a's row fully intact (no cross-tenant delete).
	if err := s.SoftDeleteByOrg("org_b", a1.ID); err != ErrNotFound {
		t.Fatalf("cross-tenant delete err=%v want=ErrNotFound", err)
	}
	if _, ok := s.GetByOrg("org_a", a1.ID); !ok {
		t.Fatal("org_a's source was wrongly removed by org_b's delete attempt")
	}

	// The owner can soft-delete; afterwards the row disappears from list/get.
	if err := s.SoftDeleteByOrg("org_a", a1.ID); err != nil {
		t.Fatalf("owner delete: %v", err)
	}
	if _, ok := s.GetByOrg("org_a", a1.ID); ok {
		t.Fatal("soft-deleted source still visible via GetByOrg")
	}
	aList, _ := s.ListByOrg("org_a", 50, "")
	if len(aList) != 0 {
		t.Fatalf("soft-deleted source still in list (len=%d)", len(aList))
	}

	// Double-delete is idempotent-as-not-found (no live row matches).
	if err := s.SoftDeleteByOrg("org_a", a1.ID); err != ErrNotFound {
		t.Fatalf("re-delete err=%v want=ErrNotFound", err)
	}

	// org_b's source is untouched throughout.
	if _, ok := s.GetByOrg("org_b", b1.ID); !ok {
		t.Fatal("org_b's source was affected by org_a operations")
	}
}

func TestMemorySources_CreateConflict(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Sources()
	src := newSource("org_a", "dup", "https://a.example/x")
	if err := s.Create(src); err != nil {
		t.Fatalf("first create: %v", err)
	}
	if err := s.Create(src); err != ErrConflict {
		t.Fatalf("duplicate create err=%v want=ErrConflict", err)
	}
}

// TestMemorySources_UpsertByOrgAndURL_CollapsesDuplicates proves the
// 2026-07-20 crawl-to-KB fix: repeat upserts for the same (org_id, url) —
// e.g. one per page of a multi-page crawl of the same host — collapse into
// a single row instead of duplicating, and the second call correctly
// reports created=false while returning the ORIGINAL row's identity.
func TestMemorySources_UpsertByOrgAndURL_CollapsesDuplicates(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Sources()

	first := newSource("org_a", "example.com", "https://example.com")
	got1, created1, err := s.UpsertByOrgAndURL(first)
	if err != nil {
		t.Fatalf("first upsert: %v", err)
	}
	if !created1 {
		t.Fatal("first upsert should report created=true")
	}
	if got1.ID != first.ID {
		t.Fatalf("first upsert id=%s want=%s", got1.ID, first.ID)
	}

	// Same org + same URL, different minted ID (mirrors a second crawled page
	// on the same host calling the registrar again) — must NOT create a
	// second row.
	second := newSource("org_a", "example.com", "https://example.com")
	got2, created2, err := s.UpsertByOrgAndURL(second)
	if err != nil {
		t.Fatalf("second upsert: %v", err)
	}
	if created2 {
		t.Fatal("second upsert for the same (org_id, url) should report created=false")
	}
	if got2.ID != first.ID {
		t.Fatalf("second upsert returned a different row: id=%s want=%s (original)", got2.ID, first.ID)
	}

	list, _ := s.ListByOrg("org_a", 50, "")
	if len(list) != 1 {
		t.Fatalf("expected exactly 1 row after 2 upserts of the same URL, got %d", len(list))
	}

	// Different org, same URL — must NOT collapse across tenants.
	other := newSource("org_b", "example.com", "https://example.com")
	_, created3, err := s.UpsertByOrgAndURL(other)
	if err != nil {
		t.Fatalf("cross-org upsert: %v", err)
	}
	if !created3 {
		t.Fatal("upsert for a different org with the same URL must create its own row")
	}
}
