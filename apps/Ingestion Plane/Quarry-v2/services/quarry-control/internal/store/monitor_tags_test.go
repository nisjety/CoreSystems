package store

import (
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// E3.5-6: tags on monitors (schedules) and sources. The in-memory store answers
// the same tag contract as the Postgres store; pg/monitor_tags_test.go holds the
// database side against migration 027.

// newTaggedSchedule is a change monitor of org carrying tag ("" is untagged).
func newTaggedSchedule(org, tag string) Schedule {
	return Schedule{
		ID:         quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:      org,
		Cron:       "0 * * * *",
		TargetKind: TargetKindChangeMonitor,
		TargetRef:  "https://" + org + ".example/pricing",
		Enabled:    true,
		CreatedAt:  time.Now().Unix(),
		Tag:        tag,
	}
}

func TestNormalizeTag_StoredFormAndRefusals(t *testing.T) {
	t.Parallel()
	valid := []struct{ in, want string }{
		{"", ""},
		{"   ", ""},
		{"Pricing", "pricing"},
		{"  EU-west.1 ", "eu-west.1"},
		{"a", "a"},
		{"0_x", "0_x"},
		{strings.Repeat("a", 32), strings.Repeat("a", 32)},
	}
	for _, c := range valid {
		got, err := NormalizeTag(c.in)
		if err != nil || got != c.want {
			t.Fatalf("NormalizeTag(%q) = %q, %v; want %q, nil", c.in, got, err, c.want)
		}
	}
	refused := []string{"-x", ".x", "_x", "a b", "a/b", "a:b", "café", strings.Repeat("a", 33)}
	for _, in := range refused {
		_, err := NormalizeTag(in)
		var ve *ValidationError
		if !errors.As(err, &ve) || ve.Reason != "invalid_tag" {
			t.Fatalf("NormalizeTag(%q) err = %v; want a ValidationError with reason invalid_tag", in, err)
		}
		if strings.Contains(ve.Message, in) {
			t.Fatalf("NormalizeTag(%q) message echoes the input: %q", in, ve.Message)
		}
	}
}

func TestMemoryMonitorTags_ListByTagIsOrgScopedAndNormalised(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Schedules()

	a1 := newTaggedSchedule("org_a", "Pricing")
	a2 := newTaggedSchedule("org_a", "")
	b1 := newTaggedSchedule("org_b", "pricing")
	for _, sch := range []Schedule{a1, a2, b1} {
		if err := s.Create(sch); err != nil {
			t.Fatalf("create %s: %v", sch.ID, err)
		}
	}

	// The write and the filter both normalise, so "Pricing" and "PRICING" are one tag.
	got, _ := s.ListByOrgTag("org_a", "PRICING", 50, "")
	if len(got) != 1 || got[0].ID != a1.ID || got[0].Tag != "pricing" {
		t.Fatalf("org_a tag list = %+v; want only a1 with tag pricing", got)
	}
	// org_b's row carries the same tag and is not in org_a's list.
	gotB, _ := s.ListByOrgTag("org_b", "pricing", 50, "")
	if len(gotB) != 1 || gotB[0].ID != b1.ID {
		t.Fatalf("org_b tag list = %+v; want only b1", gotB)
	}
	// The empty tag lists the org's untagged schedules.
	untagged, _ := s.ListByOrgTag("org_a", "", 50, "")
	if len(untagged) != 1 || untagged[0].ID != a2.ID {
		t.Fatalf("org_a untagged list = %+v; want only a2", untagged)
	}
	// An empty org matches nothing, and so does a tag that does not normalise.
	if got, _ := s.ListByOrgTag("", "pricing", 50, ""); len(got) != 0 {
		t.Fatalf("an empty org matched %d rows", len(got))
	}
	if got, _ := s.ListByOrgTag("org_a", "not a tag", 50, ""); len(got) != 0 {
		t.Fatalf("an invalid tag matched %d rows", len(got))
	}
	// A refused tag is never stored.
	if err := s.Create(newTaggedSchedule("org_a", "not a tag")); err == nil {
		t.Fatal("a schedule with an invalid tag was created")
	}
}

func TestMemoryMonitorTags_UpdateIsOrgScoped(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Schedules()
	a1 := newTaggedSchedule("org_a", "pricing")
	if err := s.Create(a1); err != nil {
		t.Fatalf("create: %v", err)
	}

	// org_b cannot retag org_a's monitor, and the row keeps its tag.
	if err := s.UpdateTagByOrg(a1.ID, "org_b", "ops"); err != ErrNotFound {
		t.Fatalf("cross-tenant retag err = %v; want ErrNotFound", err)
	}
	if got, _ := s.GetByOrg(a1.ID, "org_a"); got.Tag != "pricing" {
		t.Fatalf("tag after a refused cross-tenant retag = %q; want pricing", got.Tag)
	}

	// The owner retags, and the filter sees the new tag only.
	if err := s.UpdateTagByOrg(a1.ID, "org_a", "Ops"); err != nil {
		t.Fatalf("owner retag: %v", err)
	}
	if got, _ := s.ListByOrgTag("org_a", "pricing", 50, ""); len(got) != 0 {
		t.Fatalf("the old tag still lists %d rows", len(got))
	}
	if got, _ := s.ListByOrgTag("org_a", "ops", 50, ""); len(got) != 1 || got[0].ID != a1.ID {
		t.Fatalf("the new tag list = %+v; want a1", got)
	}

	// A refused tag leaves the row alone; an empty tag clears it.
	var ve *ValidationError
	if err := s.UpdateTagByOrg(a1.ID, "org_a", "not a tag"); !errors.As(err, &ve) || ve.Reason != "invalid_tag" {
		t.Fatalf("invalid retag err = %v; want invalid_tag", err)
	}
	if err := s.UpdateTagByOrg(a1.ID, "org_a", ""); err != nil {
		t.Fatalf("clear tag: %v", err)
	}
	if got, _ := s.GetByOrg(a1.ID, "org_a"); got.Tag != "" {
		t.Fatalf("cleared tag = %q; want empty", got.Tag)
	}
	// An empty org never updates anything.
	if err := s.UpdateTagByOrg(a1.ID, "", "ops"); err != ErrNotFound {
		t.Fatalf("empty-org retag err = %v; want ErrNotFound", err)
	}
}

func TestMemorySourceTags_ListUpdateAndSoftDeleteAreOrgScoped(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Sources()

	a1 := newSource("org_a", "A pricing", "https://a.example/pricing")
	a1.Tag = "pricing"
	a2 := newSource("org_a", "A blog", "https://a.example/blog")
	b1 := newSource("org_b", "B pricing", "https://b.example/pricing")
	b1.Tag = "pricing"
	for _, src := range []Source{a1, a2, b1} {
		if err := s.Create(src); err != nil {
			t.Fatalf("create %s: %v", src.ID, err)
		}
	}
	if got, _ := s.ListByOrgTag("org_a", "Pricing", 50, ""); len(got) != 1 || got[0].ID != a1.ID {
		t.Fatalf("org_a source tag list = %+v; want only a1", got)
	}
	if got, _ := s.ListByOrgTag("org_b", "pricing", 50, ""); len(got) != 1 || got[0].ID != b1.ID {
		t.Fatalf("org_b source tag list = %+v; want only b1", got)
	}

	// org_b cannot retag org_a's source.
	if err := s.UpdateTagByOrg("org_b", a1.ID, "ops"); err != ErrNotFound {
		t.Fatalf("cross-tenant source retag err = %v; want ErrNotFound", err)
	}
	if got, _ := s.GetByOrg("org_a", a1.ID); got.Tag != "pricing" {
		t.Fatalf("tag after a refused source retag = %q; want pricing", got.Tag)
	}

	// The owner retags a2, and updated_at does not go backwards.
	before, _ := s.GetByOrg("org_a", a2.ID)
	if err := s.UpdateTagByOrg("org_a", a2.ID, "Ops"); err != nil {
		t.Fatalf("owner retag: %v", err)
	}
	after, _ := s.GetByOrg("org_a", a2.ID)
	if after.Tag != "ops" || after.UpdatedAt < before.UpdatedAt {
		t.Fatalf("retagged source = tag %q updated_at %d (was %d)", after.Tag, after.UpdatedAt, before.UpdatedAt)
	}

	// A refused tag leaves the row alone.
	var ve *ValidationError
	if err := s.UpdateTagByOrg("org_a", a2.ID, "not a tag"); !errors.As(err, &ve) || ve.Reason != "invalid_tag" {
		t.Fatalf("invalid source retag err = %v; want invalid_tag", err)
	}

	// A soft-deleted source leaves the tag list and cannot be retagged.
	if err := s.SoftDeleteByOrg("org_a", a1.ID); err != nil {
		t.Fatalf("soft delete: %v", err)
	}
	if got, _ := s.ListByOrgTag("org_a", "pricing", 50, ""); len(got) != 0 {
		t.Fatalf("a soft-deleted source is still listed by tag: %+v", got)
	}
	if err := s.UpdateTagByOrg("org_a", a1.ID, "ops"); err != ErrNotFound {
		t.Fatalf("retag of a soft-deleted source err = %v; want ErrNotFound", err)
	}
}

func TestMemorySourceTags_RepeatRegistrationKeepsTheTag(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	s := db.Sources()

	first := newSource("org_a", "A", "https://a.example/")
	first.Tag = "pricing"
	got, created, err := s.UpsertByOrgAndURL(first)
	if err != nil || !created || got.Tag != "pricing" {
		t.Fatalf("first upsert = %+v created=%v err=%v; want tag pricing, created", got, created, err)
	}
	again := newSource("org_a", "A", "https://a.example/")
	again.Tag = "ops"
	got2, created2, err2 := s.UpsertByOrgAndURL(again)
	if err2 != nil || created2 || got2.ID != first.ID || got2.Tag != "pricing" {
		t.Fatalf("repeat upsert = %+v created=%v err=%v; want the first row with tag pricing", got2, created2, err2)
	}
}
