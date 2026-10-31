package tenderdocs

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/match"
)

func TestEvidenceTurnsWhatIsStoredIntoWhatTheGateReads(t *testing.T) {
	f := newFixture(t)
	f.store.evidence["2026-100001"] = docstore.Evidence{
		Documents: 3, Unread: 1,
		Findings: []docstore.Finding{
			{Document: "kg.pdf", Capability: "iso9001", Class: "requirement", Page: 3, Snippet: "Leverandøren skal ha ISO 9001."},
			{Document: "kontrakt.docx", Capability: "startbank", Class: "award", Paragraph: 7, Snippet: "Poeng for StartBANK."},
		},
	}
	got, err := f.svc.Evidence(context.Background(), testOrg, []string{"2026-100001", "2026-100002"})
	if err != nil {
		t.Fatalf("Evidence: %v", err)
	}
	want := map[string]match.Evidence{"2026-100001": {
		Documents: 3, Unread: 1,
		Findings: []match.Finding{
			{Capability: match.CapISO9001, Class: match.ClassRequirement, Document: "kg.pdf", Page: 3, Snippet: "Leverandøren skal ha ISO 9001."},
			{Capability: match.CapStartBank, Class: match.ClassAward, Document: "kontrakt.docx", Paragraph: 7, Snippet: "Poeng for StartBANK."},
		},
	}}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("evidence = %+v\nwant %+v", got, want)
	}
}

// A finding that this version of the rules cannot judge is left out: a capability it has never heard
// of would be judged with no lead time at all, which is the wrong answer to "can the company get it".
func TestEvidenceLeavesOutFindingsItCannotJudge(t *testing.T) {
	f := newFixture(t)
	f.store.evidence["2026-100001"] = docstore.Evidence{Documents: 1, Findings: []docstore.Finding{
		{Document: "a.pdf", Capability: "a_capability_from_the_future", Class: "requirement", Page: 1, Snippet: "x"},
		{Document: "a.pdf", Capability: "iso9001", Class: "a_class_from_the_future", Page: 1, Snippet: "y"},
		{Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 2, Snippet: "z"},
	}}
	got, err := f.svc.Evidence(context.Background(), testOrg, []string{"2026-100001"})
	if err != nil {
		t.Fatal(err)
	}
	if fs := got["2026-100001"].Findings; len(fs) != 1 || fs[0].Snippet != "z" {
		t.Errorf("findings = %+v, want only the one that can be judged", fs)
	}
	if got["2026-100001"].Documents != 1 {
		t.Errorf("the counts must survive: %+v", got["2026-100001"])
	}
}

func TestEvidenceAsksForEachNoticeOnceAndInBatches(t *testing.T) {
	f := newFixture(t)
	var ids []string
	for i := 0; i < 1200; i++ {
		ids = append(ids, fmt.Sprintf("2026-%06d", 100000+i))
	}
	ids = append(ids, ids[0], ids[1]) // repeats
	if _, err := f.svc.Evidence(context.Background(), testOrg, ids); err != nil {
		t.Fatal(err)
	}
	total := 0
	for _, n := range f.store.evidenceN {
		if n > docstore.MaxEvidenceNotices {
			t.Errorf("asked about %d notices at once, the store takes %d", n, docstore.MaxEvidenceNotices)
		}
		total += n
	}
	if total != 1200 || len(f.store.evidenceN) != 3 {
		t.Errorf("asked about %d notices in %d calls, want 1200 in 3", total, len(f.store.evidenceN))
	}
}

func TestEvidenceAsksNothingOfAnEmptyListOrAnEmptyOrganisation(t *testing.T) {
	f := newFixture(t)
	for name, tc := range map[string]struct {
		org string
		ids []string
	}{
		"no notices":      {testOrg, nil},
		"no organisation": {"", []string{"2026-100001"}},
	} {
		got, err := f.svc.Evidence(context.Background(), tc.org, tc.ids)
		if err != nil || len(got) != 0 {
			t.Errorf("%s: %v, %v", name, got, err)
		}
	}
	if f.store.callCount("EvidenceFor") != 0 {
		t.Errorf("the store was asked when there was nothing to ask")
	}
}

func TestEvidencePassesOnAFailure(t *testing.T) {
	f := newFixture(t)
	f.store.failWith["EvidenceFor"] = errors.New("the database is down")
	if _, err := f.svc.Evidence(context.Background(), testOrg, []string{"2026-100001"}); err == nil {
		t.Errorf("a failure of the store is lost")
	}
}

func TestListPassesOnWhatTheOrganisationHolds(t *testing.T) {
	f := newFixture(t)
	if _, _, err := f.upload(t, zipOf(t, file("a.txt", "Leverandøren skal ha ISO 9001."))); err != nil {
		t.Fatal(err)
	}
	got, err := f.svc.List(context.Background(), testOrg, testNotice)
	if err != nil || len(got) != 1 {
		t.Fatalf("List = %d, %v", len(got), err)
	}
	if none, err := f.svc.List(context.Background(), "org-b", testNotice); err != nil || len(none) != 0 {
		t.Errorf("another organisation's List = %d, %v", len(none), err)
	}
}

func TestDelete(t *testing.T) {
	f := newFixture(t)
	up, _, err := f.upload(t, zipOf(t, file("a.txt", "x")))
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if err := f.svc.Delete(ctx, "org-b", testNotice, up.ID); !errors.Is(err, ErrNotFound) {
		t.Errorf("another organisation's delete: err = %v, want ErrNotFound", err)
	}
	// An upload belongs to one notice, and is deleted through that notice.
	if err := f.svc.Delete(ctx, testOrg, "2026-100002", up.ID); !errors.Is(err, ErrNotFound) {
		t.Errorf("a delete through another notice: err = %v, want ErrNotFound", err)
	}
	if left, _ := f.store.List(ctx, testOrg, testNotice); len(left) != 1 {
		t.Errorf("%d uploads are left: the delete through another notice, or by another organisation, removed one", len(left))
	}
	if err := f.svc.Delete(ctx, testOrg, testNotice, up.ID); err != nil {
		t.Errorf("the owner's delete: %v", err)
	}
	if err := f.svc.Delete(ctx, testOrg, testNotice, up.ID); !errors.Is(err, ErrNotFound) {
		t.Errorf("a second delete: err = %v, want ErrNotFound", err)
	}
}

func TestRunPurgeRemovesTheExpiredAtEveryTickAndStopsWhenTheContextEnds(t *testing.T) {
	f := newFixture(t)
	f.store.purged = 2
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { f.svc.RunPurge(ctx, 10*time.Millisecond); close(done) }()

	deadline := time.After(10 * time.Second)
	for f.store.callCount("PurgeExpired") < 3 {
		select {
		case <-deadline:
			t.Fatalf("purged %d times in 10 seconds", f.store.callCount("PurgeExpired"))
		case <-time.After(5 * time.Millisecond):
		}
	}
	cancel()
	select {
	case <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("RunPurge did not stop when its context ended")
	}
	if !strings.Contains(f.logs.String(), `"removed":2`) {
		t.Errorf("a purge that removed rows is not logged: %s", f.logs.String())
	}
}

// An upload that goes between the listing and the delete is not found, and is not reported as deleted.
func TestDeleteOfAnUploadThatWentInTheMeantimeIsNotFound(t *testing.T) {
	f := newFixture(t)
	up, _, err := f.upload(t, zipOf(t, file("a.txt", "x")))
	if err != nil {
		t.Fatal(err)
	}
	f.store.vanishOnDelete = true
	if err := f.svc.Delete(context.Background(), testOrg, testNotice, up.ID); !errors.Is(err, ErrNotFound) {
		t.Errorf("err = %v, want ErrNotFound", err)
	}
}

// The first purge is at start-up, not after the first interval: a service that restarts less often than
// it purges would otherwise never purge.
func TestRunPurgeRunsAtOnceAndNotOnlyAfterTheFirstInterval(t *testing.T) {
	f := newFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { f.svc.RunPurge(ctx, time.Hour); close(done) }()
	deadline := time.After(10 * time.Second)
	for f.store.callCount("PurgeExpired") < 1 {
		select {
		case <-deadline:
			t.Fatal("nothing was purged within 10 seconds of the start, with an interval of an hour")
		case <-time.After(5 * time.Millisecond):
		}
	}
	cancel()
	<-done
}

func TestRunPurgeSurvivesAFailureAndTriesAgain(t *testing.T) {
	f := newFixture(t)
	f.store.failWith["PurgeExpired"] = errors.New("the database is down")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go f.svc.RunPurge(ctx, 10*time.Millisecond)
	deadline := time.After(10 * time.Second)
	for f.store.callCount("PurgeExpired") < 3 {
		select {
		case <-deadline:
			t.Fatalf("purged %d times: a failure ended the loop", f.store.callCount("PurgeExpired"))
		case <-time.After(5 * time.Millisecond):
		}
	}
}
