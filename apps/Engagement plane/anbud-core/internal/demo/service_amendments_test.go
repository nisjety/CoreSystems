package demo

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
)

const (
	procedureOne = "00000000-0000-4000-8000-0000000000c1"
	originalUUID = "00000000-0000-4000-8000-00000000c001"
	changeUUID   = "00000000-0000-4000-8000-00000000c002"
)

// original is a tender issued two weeks ago with a deadline in twenty days.
func original() rawstore.Document {
	return doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{
		ID: originalUUID, ProcedureID: procedureOne, Title: "Drift av fagsystem", CPV: "72212000", NUTS: "NO081",
		Deadline: in(20), IssuedAt: in(-14),
	}))
}

// change is the notice that changes it, a week later: the deadline is fourteen days later, and it says so.
func change(mutate ...func(*eformstest.Notice)) rawstore.Document {
	n := eformstest.Notice{
		ID: changeUUID, ProcedureID: procedureOne, Title: "Drift av fagsystem", CPV: "72212000", NUTS: "NO081",
		Deadline: in(34), IssuedAt: in(-7),
		Change: &eformstest.Change{Replaces: originalUUID + "-01", Reason: "Fristen er utsatt.", Sections: []string{"LOT-0001"}},
	}
	for _, m := range mutate {
		m(&n)
	}
	return doc("2026-100002", eformstest.ContractNotice(n))
}

func ctx() context.Context { return context.Background() }

// The list shows a tender once, as it stands now: the latest notice of the procedure, judged by its
// own deadline. The earlier notice is counted as superseded, and the tender says what changed.
func TestMatchesShowsOnlyTheCurrentVersionOfAnAmendedTender(t *testing.T) {
	t.Parallel()

	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{change(), original()}}, nil).Matches(ctx(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if got.Funnel.Scanned != 2 || got.Funnel.Opportunities != 1 || got.Funnel.Superseded != 1 {
		t.Errorf("funnel = %+v, want 2 scanned, 1 opportunity and 1 superseded", got.Funnel)
	}
	if len(got.Results) != 1 || got.Results[0].NoticeID != "2026-100002" {
		t.Fatalf("results = %v, want the change notice alone", resultIDs(got.Results))
	}
	r := got.Results[0]
	if r.Deadline == nil || r.Deadline.DaysLeft != 34 {
		t.Errorf("deadline = %+v, want the current version's, 34 days away", r.Deadline)
	}
	a := r.Amendment
	if a == nil || a.Versions != 2 || a.ReasonCode != "update-add" || a.Reason != "Fristen er utsatt." || !a.Confirmed || a.LastIssued == "" {
		t.Fatalf("amendment = %+v", a)
	}
	var moved *NoticeChangeView
	for i := range a.Changes {
		if a.Changes[i].Field == "deadline" {
			moved = &a.Changes[i]
		}
	}
	if moved == nil || moved.DaysMoved == nil || *moved.DaysMoved != 14 || moved.Op != "changed" || !moved.Material {
		t.Errorf("deadline change = %+v, want 14 days later", moved)
	}
}

// What the list says does not depend on the order the store returned the rows in.
func TestMatchesOfAnAmendedTenderDoNotDependOnTheStoreOrder(t *testing.T) {
	t.Parallel()

	var first string
	for _, docs := range [][]rawstore.Document{{change(), original()}, {original(), change()}} {
		got, err := newService(t, &fakeNotices{docs: docs}, nil).Matches(ctx(), mustDecode(t, itProfile))
		if err != nil || len(got.Results) != 1 {
			t.Fatalf("Matches: %v %v", err, resultIDs(got.Results))
		}
		raw, _ := json.Marshal(got.Results[0])
		if first == "" {
			first = string(raw)
		} else if first != string(raw) {
			t.Errorf("the answer changes with the store order:\n%s\n%s", first, raw)
		}
	}
}

// A tender nobody amended has no amendment, and a tender of another procedure is not merged into it.
func TestMatchesDoNotMergeProceduresOrInventAmendments(t *testing.T) {
	t.Parallel()

	other := doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{
		ID: "00000000-0000-4000-8000-00000000c003", ProcedureID: "00000000-0000-4000-8000-0000000000c2",
		Title: "Annen drift", CPV: "72212000", NUTS: "NO081", Deadline: in(10), IssuedAt: in(-3),
	}))
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{other, change(), original()}}, nil).Matches(ctx(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if len(got.Results) != 2 || got.Funnel.Superseded != 1 {
		t.Fatalf("results = %v superseded %d, want the other tender and the change", resultIDs(got.Results), got.Funnel.Superseded)
	}
	for _, r := range got.Results {
		if r.NoticeID == "2026-100003" && r.Amendment != nil {
			t.Errorf("a tender nobody amended has an amendment: %+v", r.Amendment)
		}
	}
}

// Nothing is lost without a count: every notice read is one of the kinds the funnel names.
func TestTheFunnelStillAddsUpWithSupersededNotices(t *testing.T) {
	t.Parallel()

	docs := append([]rawstore.Document{change(), original()}, corpus()...)
	got, err := newService(t, &fakeNotices{docs: docs}, nil).Matches(ctx(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	f := got.Funnel
	if sum := f.Unreadable + f.Unsupported + f.Awards + f.Opportunities + f.Superseded + f.Inactive + f.Unscheduled; sum != f.Scanned {
		t.Errorf("funnel %+v: the parts add up to %d, scanned %d", f, sum, f.Scanned)
	}
	if f.Superseded != 1 {
		t.Errorf("superseded = %d, want 1", f.Superseded)
	}
}

// A change notice that cannot be read, or whose body has been erased, is not the current version: the
// tender is the one before it, with no amendment, and the unreadable notice is counted as such.
func TestAnUnreadableChangeLeavesTheEarlierVersionAsTheTender(t *testing.T) {
	t.Parallel()

	erased := doc("2026-100002", nil)
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{erased, original()}}, nil).Matches(ctx(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if len(got.Results) != 1 || got.Results[0].NoticeID != "2026-100001" || got.Results[0].Amendment != nil {
		t.Fatalf("results = %v, want the original with no amendment", resultIDs(got.Results))
	}
	if got.Funnel.Unreadable != 1 || got.Funnel.Superseded != 0 {
		t.Errorf("funnel = %+v", got.Funnel)
	}
}

// The audit sheet of an amended tender has its history: each notice of the procedure, oldest first,
// what each changed, why, and whether it names the one before it.
func TestAuditShowsTheHistoryOfAnAmendedTender(t *testing.T) {
	t.Parallel()

	docs := []rawstore.Document{change(func(n *eformstest.Notice) { n.Title = "Drift og vedlikehold av fagsystem" }), original()}
	svc := newService(t, &fakeNotices{docs: docs}, nil)

	cur, err := svc.Audit(ctx(), "2026-100002", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	am := cur.Amendments
	if am == nil || am.ProcedureID != procedureOne || am.Position != 2 || am.SupersededBy != "" || len(am.Versions) != 2 {
		t.Fatalf("amendments of the current version = %+v", am)
	}
	if am.Versions[0].NoticeID != "2026-100001" || am.Versions[0].Current || len(am.Versions[0].Changes) != 0 {
		t.Errorf("first version = %+v", am.Versions[0])
	}
	second := am.Versions[1]
	if second.NoticeID != "2026-100002" || !second.Current || !second.Confirmed || second.Reason != "Fristen er utsatt." || second.Issued == "" {
		t.Errorf("second version = %+v", second)
	}
	fields := map[string]bool{}
	for _, c := range second.Changes {
		fields[c.Field] = true
	}
	if !fields["deadline"] || !fields["title"] {
		t.Errorf("changes = %+v, want the deadline and the title: the audit sheet shows what the list leaves out", second.Changes)
	}

	old, err := svc.Audit(ctx(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if old.Amendments == nil || old.Amendments.Position != 1 || old.Amendments.SupersededBy != "2026-100002" {
		t.Errorf("amendments of the earlier version = %+v, want position 1 and superseded by the change", old.Amendments)
	}
}

// A tender nobody amended has no history to show.
func TestAuditOfATenderNobodyAmendedHasNoAmendments(t *testing.T) {
	t.Parallel()

	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{original()}}, nil).Audit(ctx(), "2026-100001", mustDecode(t, itProfile))
	if err != nil || got.Amendments != nil {
		t.Errorf("amendments = %+v (err %v), want none", got.Amendments, err)
	}
}

// The audited notice need not be among the newest notices read: it is read on its own, and put in the
// history with the ones that are.
func TestAuditFindsTheHistoryOfANoticeOutsideTheScanWindow(t *testing.T) {
	t.Parallel()

	req := mustDecode(t, `{"profile":{"cpv":["72000000"]},"scan":1}`)
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{change(), original()}}, nil).Audit(ctx(), "2026-100001", req)
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if got.Amendments == nil || got.Amendments.Position != 1 || got.Amendments.SupersededBy != "2026-100002" || len(got.Amendments.Versions) != 2 {
		t.Errorf("amendments = %+v", got.Amendments)
	}
}

// The buyer's words are shown, clipped; a contact person never is. The reason is free text and is
// treated like the description of the notice.
func TestTheReasonIsClippedAndNoContactPersonComesOut(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("Fristen er utsatt. ", 400)
	docs := []rawstore.Document{change(func(n *eformstest.Notice) { n.Change.Reason = long }), original()}
	svc := newService(t, &fakeNotices{docs: docs}, nil)
	matches, err := svc.Matches(ctx(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	audit, err := svc.Audit(ctx(), "2026-100002", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if n := len([]rune(matches.Results[0].Amendment.Reason)); n > maxTextRunes+1 || n < 100 {
		t.Errorf("the reason on the list has %d characters", n)
	}
	if n := len([]rune(audit.Amendments.Versions[1].Reason)); n > maxTextRunes+1 || n < 100 {
		t.Errorf("the reason on the audit sheet has %d characters", n)
	}
	for _, v := range []any{matches, audit} {
		out, _ := json.Marshal(v)
		for _, marker := range eformstest.PersonalMarkers {
			if strings.Contains(string(out), marker) {
				t.Errorf("the response carries %q", marker)
			}
		}
	}
}

// The window the audit reads does not change what it judges: a change notice outside it leaves the
// tender judged as the audited notice states it.
func TestAuditStillAssessesTheNoticeThatWasAsked(t *testing.T) {
	t.Parallel()

	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{change(), original()}}, nil).Audit(ctx(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if got.Result.NoticeID != "2026-100001" || got.Result.Deadline == nil || got.Result.Deadline.DaysLeft != 20 {
		t.Errorf("result = %+v, want the original and its own deadline", got.Result)
	}
}

var _ = time.Now

// With three notices the list says what has moved since the tender was first held, not only what the
// last notice changed: the deadline went from twenty days to forty, through thirty-four, and the reason
// is the last notice's own.
func TestAmendmentOfThreeNoticesIsSinceTheFirstHeld(t *testing.T) {
	t.Parallel()

	second := change()
	second.ID = "2026-100002"
	third := doc("2026-100004", eformstest.ContractNotice(eformstest.Notice{
		ID: "00000000-0000-4000-8000-00000000c004", ProcedureID: procedureOne, Title: "Drift av fagsystem", CPV: "72212000", NUTS: "NO081",
		Deadline: in(40), IssuedAt: in(-2),
		Change: &eformstest.Change{Replaces: changeUUID + "-01", Reason: "Frist forlenget igjen.", Sections: []string{"LOT-0001"}},
	}))
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{third, second, original()}}, nil).Matches(ctx(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if len(got.Results) != 1 || got.Results[0].NoticeID != "2026-100004" || got.Funnel.Superseded != 2 {
		t.Fatalf("results %v superseded %d, want the third notice and two superseded", resultIDs(got.Results), got.Funnel.Superseded)
	}
	a := got.Results[0].Amendment
	if a == nil || a.Versions != 3 || a.Reason != "Frist forlenget igjen." {
		t.Fatalf("amendment = %+v", a)
	}
	var moved *NoticeChangeView
	for i := range a.Changes {
		if a.Changes[i].Field == "deadline" && a.Changes[i].Lot == "LOT-0001" {
			moved = &a.Changes[i]
		}
	}
	if moved == nil || moved.DaysMoved == nil || *moved.DaysMoved != 20 {
		t.Errorf("deadline change = %+v, want 20 days since the first notice (not 6 for the last change alone)", moved)
	}
}
