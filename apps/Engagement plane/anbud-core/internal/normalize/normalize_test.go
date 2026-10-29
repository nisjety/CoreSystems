package normalize

import (
	"errors"
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

func TestCPV(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		raw, want string
		err       error
	}{
		{"72000000", "72000000", nil},
		{"45000000-7", "45000000", nil},
		{" 72212000 ", "72212000", nil},
		{"7200000", "", ErrCPV},
		{"72000000-77", "", ErrCPV},
		{"7200000a", "", ErrCPV},
		{"", "", ErrCPV},
	} {
		got, err := CPV(tc.raw)
		if got != tc.want || !errors.Is(err, tc.err) {
			t.Errorf("CPV(%q) = %q, %v; want %q, %v", tc.raw, got, err, tc.want, tc.err)
		}
	}
}

func TestCPVAncestorsWidenOneLevelAtATime(t *testing.T) {
	t.Parallel()

	for code, want := range map[string][]string{
		"72212100": {"72212000", "72210000", "72200000", "72000000"},
		"72212000": {"72210000", "72200000", "72000000"},
		"72200000": {"72000000"},
		"72000000": nil,
		"03111110": {"03111100", "03111000", "03110000", "03100000", "03000000"},
	} {
		if got := CPVAncestors(code); !slices.Equal(got, want) {
			t.Errorf("CPVAncestors(%q) = %v, want %v", code, got, want)
		}
	}
	if got := CPVAncestors("not a code"); got != nil {
		t.Errorf("a bad code got ancestors %v", got)
	}
}

func TestNUTS(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		raw, want string
		err       error
	}{
		{"NO081", "NO081", nil},
		{"no0a3", "NO0A3", nil},
		{" NO08 ", "NO08", nil},
		{"NO", "NO", nil},
		{"NOZZZ", "", ErrNUTSExtraRegio},
		{"N0081", "", ErrNUTS},
		{"NO08100", "", ErrNUTS},
		{"", "", ErrNUTS},
	} {
		got, err := NUTS(tc.raw)
		if got != tc.want || !errors.Is(err, tc.err) {
			t.Errorf("NUTS(%q) = %q, %v; want %q, %v", tc.raw, got, err, tc.want, tc.err)
		}
	}
	if got, want := NUTSAncestors("NO081"), []string{"NO08", "NO0", "NO"}; !slices.Equal(got, want) {
		t.Errorf("NUTSAncestors = %v, want %v", got, want)
	}
	if got := NUTSAncestors("NO"); got != nil {
		t.Errorf("a country got ancestors %v", got)
	}
}

func TestOrgNumber(t *testing.T) {
	t.Parallel()

	// 974760673 is Brønnøysundregistrene's own organisation number, a published public fact; its
	// check digit is valid. 974760674 differs only in the check digit.
	for _, tc := range []struct {
		raw, want string
		err       error
	}{
		{"974760673", "974760673", nil},
		{"974 760 673", "974760673", nil},
		{"NO 974 760 673 MVA", "974760673", nil},
		{"NO974760673MVA", "974760673", nil},
		{"974.760.673", "974760673", nil},
		{"974760674", "", ErrOrgNumberChecksum},
		{"97476067", "", ErrOrgNumber},
		{"9747606731", "", ErrOrgNumber},
		{"SE5560000000", "", ErrOrgNumber},
		{"", "", ErrOrgNumber},
	} {
		got, err := OrgNumber(tc.raw)
		if got != tc.want || !errors.Is(err, tc.err) {
			t.Errorf("OrgNumber(%q) = %q, %v; want %q, %v", tc.raw, got, err, tc.want, tc.err)
		}
	}
}

// A check digit of 10 can never be valid: such a number is refused, not rounded.
func TestOrgNumberRefusesARemainderOfOne(t *testing.T) {
	t.Parallel()

	// The stem is found by search, so the test does not depend on hand arithmetic.
	for n := 0; n < 1_000_000; n++ {
		digits := []byte("0" + pad7(n))
		if checkDigit(digits) == -1 {
			for d := byte('0'); d <= '9'; d++ {
				if _, err := OrgNumber(string(digits) + string(d)); !errors.Is(err, ErrOrgNumberChecksum) {
					t.Fatalf("%s%c: err = %v, want ErrOrgNumberChecksum", digits, d, err)
				}
			}
			return
		}
	}
	t.Fatal("no eight-digit stem with remainder 1 found")
}

func pad7(n int) string {
	s := []byte("0000000")
	for i := 6; i >= 0 && n > 0; i-- {
		s[i] = byte('0' + n%10)
		n /= 10
	}
	return string(s)
}

func TestOsloDayAndDaysLeft(t *testing.T) {
	t.Parallel()

	// 23:30 UTC on 14 November is 00:30 on 15 November in Oslo (UTC+1 in winter).
	deadline := time.Date(2026, time.November, 14, 23, 30, 0, 0, time.UTC)
	if got := OsloDay(deadline); got != "2026-11-15" {
		t.Errorf("OsloDay = %q, want 2026-11-15", got)
	}
	now := time.Date(2026, time.November, 10, 22, 0, 0, 0, time.UTC) // 23:00 on 10 November in Oslo
	if got := DaysLeft(deadline, now); got != 5 {
		t.Errorf("DaysLeft = %d, want 5 Oslo calendar days", got)
	}
	if got := DaysLeft(now, deadline); got != -5 {
		t.Errorf("DaysLeft of a passed deadline = %d, want -5", got)
	}
	// Across the switch to summer time the count stays in calendar days.
	spring := time.Date(2026, time.March, 30, 10, 0, 0, 0, time.UTC)
	before := time.Date(2026, time.March, 28, 10, 0, 0, 0, time.UTC)
	if got := DaysLeft(spring, before); got != 2 {
		t.Errorf("DaysLeft across daylight saving = %d, want 2", got)
	}
}

func notice() eforms.Notice {
	d1 := time.Date(2026, time.November, 20, 11, 0, 0, 0, time.UTC)
	d2 := time.Date(2026, time.November, 18, 11, 0, 0, 0, time.UTC)
	return eforms.Notice{
		CPVMain:         []string{"45000000-7"},
		CPVAdditional:   []string{"45000000", "72212000", "bad"},
		NUTS:            []string{"no081", "NOZZZ"},
		BuyerOrgNumbers: []string{"974760673", "974760674"},
		Lots: []eforms.Lot{
			{ID: "LOT-0001", CPVMain: []string{"72212000"}, Deadline: &d1},
			{ID: "LOT-0002", Deadline: &d2},
		},
		Results: []eforms.LotResult{{Winners: []eforms.Winner{{Name: "x", OrgNumber: "NO 974 760 673 MVA"}}}},
	}
}

func TestNoticeNormalisesAndRecordsEveryChange(t *testing.T) {
	t.Parallel()

	in := notice()
	got := Notice(in)

	if !slices.Equal(got.CPVMain, []string{"45000000"}) {
		t.Errorf("CPVMain = %v", got.CPVMain)
	}
	if !slices.Equal(got.CPV, []string{"45000000", "72212000"}) {
		t.Errorf("CPV = %v, want main, additional and lot codes, each once", got.CPV)
	}
	if !slices.Equal(got.NUTS, []string{"NO081"}) {
		t.Errorf("NUTS = %v", got.NUTS)
	}
	if !slices.Equal(got.BuyerOrgNumbers, []string{"974760673"}) || !slices.Equal(got.WinnerOrgNumbers, []string{"974760673"}) {
		t.Errorf("org numbers buyer %v winner %v", got.BuyerOrgNumbers, got.WinnerOrgNumbers)
	}
	if want := time.Date(2026, time.November, 18, 11, 0, 0, 0, time.UTC); got.EarliestDeadline == nil || !got.EarliestDeadline.Equal(want) {
		t.Errorf("EarliestDeadline = %v, want the earliest lot deadline %v", got.EarliestDeadline, want)
	}

	wantChanges := []Change{
		{Field: "cpv", From: "45000000-7", To: "45000000", Rule: RuleCPVCheckDigit},
		{Field: "nuts", From: "no081", To: "NO081", Rule: RuleUpperCase},
		{Field: "winner org number", From: "NO 974 760 673 MVA", To: "974760673", Rule: RuleOrgNumberForm},
	}
	if !slices.Equal(got.Changes, wantChanges) {
		t.Errorf("changes =\n%+v\nwant\n%+v", got.Changes, wantChanges)
	}
	wantProblems := []string{
		"cpv: a code is not a CPV code",
		"nuts: a code is extra-regio, not a place",
		"buyer org number: the check digit is wrong",
	}
	if !slices.Equal(got.Problems, wantProblems) {
		t.Errorf("problems = %q, want %q", got.Problems, wantProblems)
	}

	// The input is not changed.
	if in.CPVMain[0] != "45000000-7" || in.NUTS[0] != "no081" {
		t.Errorf("Notice changed its input: %v %v", in.CPVMain, in.NUTS)
	}
}

func TestNoticeTakesTheNoticeDeadlineOnlyWhenEarlier(t *testing.T) {
	t.Parallel()

	n := notice()
	early := time.Date(2026, time.November, 1, 11, 0, 0, 0, time.UTC)
	n.Deadline = &early
	if got := Notice(n).EarliestDeadline; got == nil || !got.Equal(early) {
		t.Errorf("EarliestDeadline = %v, want %v", got, early)
	}
	n.Deadline, n.Lots = nil, nil
	if got := Notice(n).EarliestDeadline; got != nil {
		t.Errorf("EarliestDeadline = %v, want none: a deadline is never invented", got)
	}
}

func day(d int) *time.Time {
	t := time.Date(2026, time.November, d, 11, 0, 0, 0, time.UTC)
	return &t
}

// A company must take the first step a notice states: when a lot has a deadline for requests to
// participate, that is the one that binds, because tenders are invited only from those who asked.
func TestDeadlinesFollowTheStepACompanyMustTakeFirst(t *testing.T) {
	t.Parallel()

	got := Notice(eforms.Notice{
		Deadline: day(30),
		Lots: []eforms.Lot{
			{ID: "A", Deadline: day(20)},
			{ID: "B", ParticipationDeadline: day(12)},
			{ID: "C", Deadline: day(25), ParticipationDeadline: day(14)},
			{ID: "D"},
		},
	})
	want := []Deadline{
		{Lot: "", Kind: DeadlineTender, At: *day(30)},
		{Lot: "A", Kind: DeadlineTender, At: *day(20)},
		{Lot: "B", Kind: DeadlineParticipation, At: *day(12)},
		{Lot: "C", Kind: DeadlineParticipation, At: *day(14)},
	}
	if !slices.Equal(got.Deadlines, want) {
		t.Errorf("deadlines =\n%+v\nwant\n%+v", got.Deadlines, want)
	}
	if got.EarliestDeadline == nil || !got.EarliestDeadline.Equal(*day(12)) {
		t.Errorf("EarliestDeadline = %v, want day 12", got.EarliestDeadline)
	}

	n := eforms.Notice{ParticipationDeadline: day(9), Deadline: day(30)}
	if d := Notice(n).Deadlines; len(d) != 1 || d[0].Kind != DeadlineParticipation || !d[0].At.Equal(*day(9)) {
		t.Errorf("notice-level deadlines = %+v, want the participation deadline", d)
	}
}

// A buyer or winner in another country has another kind of registry number. It is not kept as an
// organisation number, and the problem names the field, not the value.
func TestAForeignIdentifierIsNotANorwegianOrganisationNumber(t *testing.T) {
	t.Parallel()

	n := eforms.Notice{
		BuyerOrgNumbers: []string{"556000-0000", "556000-0001", "974760673"},
		Results:         []eforms.LotResult{{Winners: []eforms.Winner{{OrgNumber: "FI1234567-8"}, {OrgNumber: ""}}}},
	}
	got := Notice(n)
	if !slices.Equal(got.BuyerOrgNumbers, []string{"974760673"}) || len(got.WinnerOrgNumbers) != 0 {
		t.Errorf("buyers %v winners %v", got.BuyerOrgNumbers, got.WinnerOrgNumbers)
	}
	want := []string{
		"buyer org number: not a Norwegian organisation number",
		"winner org number: not a Norwegian organisation number",
	}
	if !slices.Equal(got.Problems, want) {
		t.Errorf("problems = %q, want %q", got.Problems, want)
	}
}

// A stem whose weighted sum is a multiple of 11 has check digit 0.
func TestOrgNumberAcceptsCheckDigitZero(t *testing.T) {
	t.Parallel()

	for n := 1; n < 1_000_000; n++ {
		stem := "0" + pad7(n)
		sum := 0
		for i := range 8 {
			sum += int(stem[i]-'0') * orgWeights[i]
		}
		if sum%11 != 0 {
			continue
		}
		if got, err := OrgNumber(stem + "0"); err != nil || got != stem+"0" {
			t.Fatalf("OrgNumber(%s0) = %q, %v; want it valid", stem, got, err)
		}
		return
	}
	t.Fatal("no stem with a sum divisible by 11 found")
}
