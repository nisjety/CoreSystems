package demo

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
)

const itProfile = `{"profile":{"cpv":["72000000"],"nuts":["NO08"]}}`

// corpus is six stored bodies, newest first: an open IT tender, a closed one, one for farming, a
// result notice, a body that is not a notice, and a notice of a shape no field map covers.
func corpus() []rawstore.Document {
	return []rawstore.Document{
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "Åpen drift", CPV: "72212000", NUTS: "NO081", ValueNOK: 1_000_000, Deadline: in(20)})),
		doc("2026-100002", eformstest.ContractNotice(eformstest.Notice{Title: "Stengt drift", CPV: "72212000", NUTS: "NO081", Deadline: in(-5)})),
		doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{Title: "Jordbruk", CPV: "03000000", NUTS: "NO081", Deadline: in(10)})),
		doc("2026-100004", eformstest.AwardNotice(eformstest.Notice{Title: "Tildelt drift", CPV: "72212000"})),
		doc("2026-100005", []byte("this is not a notice")),
		doc("2026-100006", eformstest.ContractNotice(eformstest.Notice{SDK: "1.7", Title: "Gammel", CPV: "72212000"})),
	}
}

func TestMatchesAssessesEveryOpportunityAndCountsTheFunnel(t *testing.T) {
	t.Parallel()

	s := newService(t, &fakeNotices{docs: corpus()}, nil)
	got, err := s.Matches(context.Background(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	want := Funnel{
		Scanned: 6, Unreadable: 1, Unsupported: 1, Awards: 1, Opportunities: 2, Inactive: 1,
		Gate:    map[string]int{"pass": 2, "conditional": 0, "fail": 0},
		Matched: 1,
	}
	if got.Funnel.Scanned != want.Scanned || got.Funnel.Unreadable != want.Unreadable || got.Funnel.Unsupported != want.Unsupported ||
		got.Funnel.Awards != want.Awards || got.Funnel.Opportunities != want.Opportunities || got.Funnel.Matched != want.Matched ||
		got.Funnel.Gate["pass"] != 2 || got.Funnel.Gate["conditional"] != 0 || got.Funnel.Gate["fail"] != 0 {
		t.Errorf("funnel = %+v, want %+v", got.Funnel, want)
	}
	if !slices.Equal(resultIDs(got.Results), []string{"2026-100001"}) {
		t.Errorf("results = %v, want only the open tender that fits", resultIDs(got.Results))
	}
	if !got.AsOf.Equal(now) || got.Strictness != "balanced" || got.Company != nil {
		t.Errorf("as of %v strictness %q company %v", got.AsOf, got.Strictness, got.Company)
	}
}

func TestMatchesWithShowAllListsTheClosedAndTheUnrelatedToo(t *testing.T) {
	t.Parallel()

	s := newService(t, &fakeNotices{docs: corpus()}, nil)
	got, err := s.Matches(context.Background(), mustDecode(t, `{"profile":{"cpv":["72000000"]},"show":"all"}`))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	// A tender a company could bid on comes first, a notice that does not fit next, and one whose
	// deadline has passed last: it fits, but nobody can act on it.
	if want := []string{"2026-100001", "2026-100003", "2026-100002"}; !slices.Equal(resultIDs(got.Results), want) {
		t.Errorf("results = %v, want %v", resultIDs(got.Results), want)
	}
	closed := got.Results[2]
	if closed.Verdict != "fail" || closed.Band != "strong" || closed.Deadline != nil {
		t.Errorf("closed = verdict %q band %q deadline %v", closed.Verdict, closed.Band, closed.Deadline)
	}
	if got.Funnel.Matched != 3 {
		t.Errorf("matched = %d, want 3 with show all", got.Funnel.Matched)
	}
}

func TestMatchesPutsTheNearestDeadlineFirstWithinABand(t *testing.T) {
	t.Parallel()

	docs := []rawstore.Document{
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{CPV: "72212000", Deadline: in(20)})),
		doc("2026-100002", eformstest.ContractNotice(eformstest.Notice{CPV: "72212000", Deadline: in(5)})),
		doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{CPV: "72250000", Deadline: in(1)})), // a sibling group: good, not strong
		doc("2026-100004", eformstest.ContractNotice(eformstest.Notice{CPV: "72212000", Deadline: in(5)})),
	}
	got, err := newService(t, &fakeNotices{docs: docs}, nil).Matches(context.Background(),
		mustDecode(t, `{"profile":{"cpv":["72212000"]},"strictness":"balanced"}`))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	// Strong before good, nearest deadline first, and the notice id breaks a tie.
	if want := []string{"2026-100002", "2026-100004", "2026-100001", "2026-100003"}; !slices.Equal(resultIDs(got.Results), want) {
		t.Errorf("results = %v, want %v", resultIDs(got.Results), want)
	}
}

func TestMatchesCapsTheResultsAndSaysHowManyMatched(t *testing.T) {
	t.Parallel()

	var docs []rawstore.Document
	for _, id := range []string{"2026-100001", "2026-100002", "2026-100003", "2026-100004", "2026-100005"} {
		docs = append(docs, doc(id, eformstest.ContractNotice(eformstest.Notice{CPV: "72212000", Deadline: in(10)})))
	}
	got, err := newService(t, &fakeNotices{docs: docs}, nil).Matches(context.Background(), mustDecode(t, `{"profile":{"cpv":["72000000"]},"limit":2}`))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if len(got.Results) != 2 || got.Funnel.Matched != 5 {
		t.Errorf("results %d matched %d, want 2 returned of 5 matched", len(got.Results), got.Funnel.Matched)
	}
}

func TestMatchesIgnoresLegacyScanLimitForFullCoverage(t *testing.T) {
	t.Parallel()

	store := &fakeNotices{docs: corpus()}
	got, err := newService(t, store, nil).Matches(context.Background(), mustDecode(t, `{"profile":{"cpv":["72000000"]},"scan":2}`))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if got.Funnel.Scanned != len(corpus()) || got.Coverage != "all_active" {
		t.Errorf("scanned %d, want complete corpus", got.Funnel.Scanned)
	}
}

func TestMatchesSaysWhatWideningWouldAdd(t *testing.T) {
	t.Parallel()

	docs := []rawstore.Document{
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{CPV: "72212000", Deadline: in(10)})), // inside the profile
		doc("2026-100002", eformstest.ContractNotice(eformstest.Notice{CPV: "72250000", Deadline: in(10)})), // same group
		doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{CPV: "72310000", Deadline: in(10)})), // same division
		doc("2026-100004", eformstest.ContractNotice(eformstest.Notice{CPV: "48000000", Deadline: in(10)})), // never
		doc("2026-100005", eformstest.ContractNotice(eformstest.Notice{CPV: "72250000", Deadline: in(-3)})), // would fit, but closed
	}
	s := newService(t, &fakeNotices{docs: docs}, nil)
	for level, want := range map[string]map[string]int{
		"strict":   {"balanced": 1, "wide": 2},
		"balanced": {"wide": 1},
		"wide":     {},
	} {
		got, err := s.Matches(context.Background(), mustDecode(t, `{"profile":{"cpv":["72212000"]},"strictness":"`+level+`"}`))
		if err != nil {
			t.Fatalf("%s: Matches: %v", level, err)
		}
		if len(got.GainedByWidening) != len(want) {
			t.Errorf("%s: gained = %v, want %v", level, got.GainedByWidening, want)
			continue
		}
		for k, v := range want {
			if got.GainedByWidening[k] != v {
				t.Errorf("%s: gained = %v, want %v", level, got.GainedByWidening, want)
			}
		}
	}
}

func TestMatchesChecksTheCompanyInTheRegister(t *testing.T) {
	t.Parallel()

	const profile = `{"profile":{"org_number":"NO 974 760 673 MVA","cpv":["72000000"]},"show":"all"}`
	t.Run("a bankrupt company fails every gate", func(t *testing.T) {
		t.Parallel()
		companies := &fakeCompanies{org: orgcore.Organisation{OrgNumber: "974760673", Name: "Konkursbo AS", Bankrupt: true}}
		got, err := newService(t, &fakeNotices{docs: corpus()}, companies).Matches(context.Background(), mustDecode(t, profile))
		if err != nil {
			t.Fatalf("Matches: %v", err)
		}
		if !slices.Equal(companies.calls, []string{"974760673"}) {
			t.Errorf("lookups = %v, want one, with the normalised number", companies.calls)
		}
		c := got.Company
		if c == nil || !c.Checked || c.Name != "Konkursbo AS" || c.CanBid == nil || *c.CanBid || c.OrgNumber != "974760673" {
			t.Errorf("company = %+v", c)
		}
		if got.Funnel.Gate["fail"] != 3 || got.Funnel.Gate["pass"] != 0 {
			t.Errorf("gate = %v, want all 3 opportunities failed", got.Funnel.Gate)
		}
		for _, r := range got.Results {
			if r.Verdict != "fail" {
				t.Errorf("%s: verdict %q", r.NoticeID, r.Verdict)
			}
		}
	})
	t.Run("a healthy company is checked and changes nothing", func(t *testing.T) {
		t.Parallel()
		companies := &fakeCompanies{org: orgcore.Organisation{OrgNumber: "974760673", Name: "Fungerende AS"}}
		got, err := newService(t, &fakeNotices{docs: corpus()}, companies).Matches(context.Background(), mustDecode(t, profile))
		if err != nil {
			t.Fatalf("Matches: %v", err)
		}
		if c := got.Company; c == nil || !c.Checked || c.CanBid == nil || !*c.CanBid || got.Funnel.Gate["pass"] != 2 {
			t.Errorf("company %+v gate %v", c, got.Funnel.Gate)
		}
	})
	for name, tc := range map[string]struct {
		companies CompanyLookup
		reason    string
	}{
		"not configured":      {nil, "not_configured"},
		"not in the register": {&fakeCompanies{err: orgcore.ErrNotFound}, "not_found"},
		"org-core is down":    {&fakeCompanies{err: errors.New("connection refused")}, "unavailable"},
		"credential refused":  {&fakeCompanies{err: orgcore.ErrUnauthorized}, "unavailable"},
	} {
		t.Run(name+": the demo goes on and says the register was not checked", func(t *testing.T) {
			t.Parallel()
			got, err := newService(t, &fakeNotices{docs: corpus()}, tc.companies).Matches(context.Background(), mustDecode(t, profile))
			if err != nil {
				t.Fatalf("Matches: %v", err)
			}
			if c := got.Company; c == nil || c.Checked || c.Reason != tc.reason || c.CanBid != nil || c.OrgNumber != "974760673" {
				t.Errorf("company = %+v, want unchecked, reason %q", c, tc.reason)
			}
			if got.Funnel.Gate["pass"] != 2 {
				t.Errorf("gate = %v: an unchecked register must not fail anything", got.Funnel.Gate)
			}
		})
	}
}

func TestMatchesAsksTheRegisterOnlyWhenAnOrganisationNumberIsGiven(t *testing.T) {
	t.Parallel()

	companies := &fakeCompanies{}
	got, err := newService(t, &fakeNotices{docs: corpus()}, companies).Matches(context.Background(), mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if len(companies.calls) != 0 || got.Company != nil {
		t.Errorf("lookups %v company %v", companies.calls, got.Company)
	}
}

// Contact persons are never copied by the parser, and nothing here may bring them back.
func TestMatchesCarryNoPersonalData(t *testing.T) {
	t.Parallel()

	got, err := newService(t, &fakeNotices{docs: corpus()}, nil).Matches(context.Background(), mustDecode(t, `{"profile":{"cpv":["72000000"]},"show":"all"}`))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	out, _ := json.Marshal(got)
	for _, marker := range eformstest.PersonalMarkers {
		if strings.Contains(string(out), marker) {
			t.Errorf("the response carries %q", marker)
		}
	}
}

func TestMatchesRefusesToRunTooManyScansAtOnce(t *testing.T) {
	t.Parallel()

	block, entered := make(chan struct{}), make(chan struct{}, 1)
	store := &blockingNotices{fakeNotices: fakeNotices{docs: corpus()}, block: block, entered: entered}
	s, err := New(Options{Notices: store, Now: func() time.Time { return now }, MaxConcurrent: 1})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	// The request is decoded here: t.Fatalf in the goroutine would leave this test waiting for ever.
	request := mustDecode(t, itProfile)
	done := make(chan error, 1)
	go func() {
		_, err := s.Matches(context.Background(), request)
		done <- err
	}()
	<-entered

	if _, err := s.Matches(context.Background(), request); !errors.Is(err, ErrBusy) {
		t.Errorf("second scan: err = %v, want ErrBusy", err)
	}
	close(block)
	if err := <-done; err != nil {
		t.Errorf("first scan: %v", err)
	}
	if _, err := s.Matches(context.Background(), request); err != nil {
		t.Errorf("after the first scan ended: %v", err)
	}
}

// blockingNotices holds a scan open until released.
type blockingNotices struct {
	fakeNotices
	block   chan struct{}
	entered chan struct{}
}

func (b *blockingNotices) RecentNotices(ctx context.Context, limit int, fn func(rawstore.Document) error) error {
	select {
	case b.entered <- struct{}{}:
		<-b.block
	default: // later scans do not block
	}
	return b.fakeNotices.RecentNotices(ctx, limit, fn)
}

type hangingNotices struct{ fakeNotices }

func (h *hangingNotices) RecentNotices(ctx context.Context, _ int, _ func(rawstore.Document) error) error {
	<-ctx.Done()
	return ctx.Err()
}

func TestMatchesStopsAtItsOwnTimeoutButNotAtTheCallersCancel(t *testing.T) {
	t.Parallel()

	s, err := New(Options{Notices: &hangingNotices{}, Now: func() time.Time { return now }, ScanTimeout: 30 * time.Millisecond})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	if _, err := s.Matches(context.Background(), mustDecode(t, itProfile)); !errors.Is(err, ErrTimeout) {
		t.Errorf("err = %v, want ErrTimeout", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := s.Matches(ctx, mustDecode(t, itProfile)); !errors.Is(err, context.Canceled) || errors.Is(err, ErrTimeout) {
		t.Errorf("a cancelled caller: err = %v, want context.Canceled", err)
	}
}

func TestMatchesReturnsAStoreErrorAsIt(t *testing.T) {
	t.Parallel()

	boom := errors.New("database is down")
	_, err := newService(t, &fakeNotices{err: boom}, nil).Matches(context.Background(), mustDecode(t, itProfile))
	if !errors.Is(err, boom) || errors.Is(err, ErrTimeout) || errors.Is(err, ErrBusy) {
		t.Errorf("err = %v, want the store's error", err)
	}
}

func TestMatchesGivesTheSameAnswerForTheSameInputs(t *testing.T) {
	t.Parallel()

	s := newService(t, &fakeNotices{docs: corpus()}, nil)
	r := mustDecode(t, `{"profile":{"cpv":["72000000"]},"show":"all"}`)
	first, _ := s.Matches(context.Background(), r)
	second, _ := s.Matches(context.Background(), r)
	a, _ := json.Marshal(first)
	b, _ := json.Marshal(second)
	if string(a) != string(b) {
		t.Errorf("two identical requests gave different answers")
	}
}

func TestNewNeedsANoticeSource(t *testing.T) {
	t.Parallel()

	if _, err := New(Options{}); err == nil {
		t.Errorf("New accepted no notice source")
	}
	s, err := New(Options{Notices: &fakeNotices{}})
	if err != nil || s == nil {
		t.Fatalf("New with defaults: %v", err)
	}
	if _, err := s.Matches(context.Background(), Request{Profile: match.Profile{CPV: []string{"72000000"}}, Limit: 5, Scan: 5}); err != nil {
		t.Errorf("default clock: %v", err)
	}
}

// Of two tenders that rank alike, the one that can still be acted on comes first, and a notice
// that states no deadline comes after one that does.
func TestMatchesPutsANoticeWithAnOpenDeadlineBeforeOneWithNone(t *testing.T) {
	t.Parallel()

	docs := []rawstore.Document{
		// No deadline stated: unknown, so conditional. Listed first by id.
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{CPV: "72212000"})),
		// A deadline, and a criterion no rule recognises: conditional as well.
		doc("2026-100002", eformstest.ContractNotice(eformstest.Notice{CPV: "72212000", Deadline: in(5), Criteria: []string{"Noe uvanlig"}})),
	}
	got, err := newService(t, &fakeNotices{docs: docs}, nil).Matches(context.Background(), mustDecode(t, `{"profile":{"cpv":["72212000"]}}`))
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	for _, r := range got.Results {
		if r.Verdict != "conditional" || r.Band != "strong" {
			t.Fatalf("%s: verdict %q band %q, the two must rank alike for this test to mean anything", r.NoticeID, r.Verdict, r.Band)
		}
	}
	if want := []string{"2026-100002"}; !slices.Equal(resultIDs(got.Results), want) {
		t.Errorf("results = %v, want %v", resultIDs(got.Results), want)
	}
}

// slowNotices waits out the scan's deadline and then hands over every document anyway, like a store
// that does not look at its context: only the scan itself can stop it.
type slowNotices struct{ fakeNotices }

func (s *slowNotices) RecentNotices(ctx context.Context, limit int, fn func(rawstore.Document) error) error {
	<-ctx.Done()
	return s.fakeNotices.RecentNotices(context.Background(), limit, fn)
}

func TestMatchesStopsReadingOnceItsTimeIsUp(t *testing.T) {
	t.Parallel()

	s, err := New(Options{Notices: &slowNotices{fakeNotices{docs: corpus()}}, Now: func() time.Time { return now }, ScanTimeout: 30 * time.Millisecond})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	if _, err := s.Matches(context.Background(), mustDecode(t, itProfile)); !errors.Is(err, ErrTimeout) {
		t.Errorf("err = %v, want ErrTimeout: the scan went on reading after its time was up", err)
	}
}

// A request built by hand, not decoded, is clamped to what the store allows.
func TestMatchesAlwaysReadsTheCompleteCorpusRegardlessOfLegacyScan(t *testing.T) {
	t.Parallel()

	for scan := range map[int]int{-5: DefaultScan, 0: DefaultScan, 7: 7, MaxScan: MaxScan, MaxScan + 1: MaxScan, 100_000: MaxScan} {
		store := &fakeNotices{docs: corpus()}
		r := Request{Profile: match.Profile{CPV: []string{"72000000"}}, Limit: 5, Scan: scan}
		if _, err := newService(t, store, nil).Matches(context.Background(), r); err != nil {
			t.Fatalf("scan %d: %v", scan, err)
		}
		if !slices.Equal(store.limits, []int{int(^uint(0) >> 1)}) {
			t.Errorf("scan %d: the store was asked for %v, want %d", scan, store.limits, int(^uint(0)>>1))
		}
	}
}
