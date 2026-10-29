package retention

import (
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
)

func day(s string) time.Time {
	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		panic(err)
	}
	return t
}

var fetched = day("2024-01-10")

// An award counts from the end of the contract it awards: the award day plus the stated duration.
// The body expires the period after that, on the day itself and not a day before.
func TestAnAwardCountsFromTheEndOfItsContract(t *testing.T) {
	body := eformstest.AwardWithResult(eformstest.Notice{}, eformstest.Award{
		WinnerName: "Leverandør AS", WinnerOrg: "923609016", Tenders: 3, AwardDate: day("2024-03-01"), DurationMonths: 24,
	})
	end := day("2026-03-01")
	for _, tc := range []struct {
		asOf    time.Time
		expired bool
	}{
		{day("2027-02-28"), false},
		{day("2027-03-01"), true},
	} {
		got, err := Decide(body, fetched, 12, tc.asOf)
		if err != nil {
			t.Fatalf("Decide: %v", err)
		}
		if got.Basis != BasisContractEnd || !got.From.Equal(end) || !got.ExpiresAt.Equal(day("2027-03-01")) || got.Expired != tc.expired {
			t.Errorf("as of %s: %+v, want contract_end from %s, expiring 2027-03-01, expired %v", tc.asOf.Format("2006-01-02"), got, end.Format("2006-01-02"), tc.expired)
		}
	}
}

// A call for tenders counts from its deadline, not from the day it was fetched.
func TestACallForTendersCountsFromItsDeadline(t *testing.T) {
	body := eformstest.ContractNotice(eformstest.Notice{Deadline: day("2024-02-15"), IssuedAt: day("2024-01-05")})
	got, err := Decide(body, fetched, 6, day("2024-08-15"))
	if err != nil {
		t.Fatalf("Decide: %v", err)
	}
	if got.Basis != BasisDeadline || !got.From.Equal(day("2024-02-15")) || !got.Expired {
		t.Errorf("%+v, want deadline from 2024-02-15 and expired", got)
	}
}

// The latest date wins, whichever basis it has: a notice issued after its deadline (a change notice
// can be) counts from the issue time.
func TestTheLatestDateWins(t *testing.T) {
	body := eformstest.ContractNotice(eformstest.Notice{Deadline: day("2024-02-15"), IssuedAt: day("2024-03-01")})
	got, err := Decide(body, fetched, 12, day("2025-02-20"))
	if err != nil {
		t.Fatalf("Decide: %v", err)
	}
	if got.Basis != BasisIssued || !got.From.Equal(day("2024-03-01")) || got.Expired {
		t.Errorf("%+v, want issued from 2024-03-01 and kept", got)
	}
}

// A prior information notice is in use until the notice it announces is planned for.
func TestAPriorInformationNoticeCountsFromItsPlannedNotice(t *testing.T) {
	body := eformstest.PriorInformationNotice(eformstest.Notice{IssuedAt: day("2024-01-05"), PlannedNotice: day("2024-09-01")})
	got, err := Decide(body, fetched, 6, day("2025-02-28"))
	if err != nil {
		t.Fatalf("Decide: %v", err)
	}
	if got.Basis != BasisPlanned || !got.From.Equal(day("2024-09-01")) || got.Expired {
		t.Errorf("%+v, want planned from 2024-09-01 and kept until 2025-03-01", got)
	}
}

// A body the parser refuses counts from the day it was fetched, and is not kept for ever.
func TestAnUnreadableBodyCountsFromItsFetch(t *testing.T) {
	got, err := Decide([]byte("<NotANotice/>"), fetched, 12, day("2025-01-10"))
	if err != nil {
		t.Fatalf("Decide: %v", err)
	}
	if got.Basis != BasisFetched || !got.From.Equal(fetched) || !got.Expired {
		t.Errorf("%+v, want fetched from %s and expired", got, fetched.Format("2006-01-02"))
	}
}

// A notice that parses but states no date also counts from the fetch.
func TestANoticeWithNoDateCountsFromItsFetch(t *testing.T) {
	got, err := decide(eforms.Notice{}, true, fetched, 12, day("2024-06-01"))
	if err != nil {
		t.Fatalf("decide: %v", err)
	}
	if got.Basis != BasisFetched || got.Expired {
		t.Errorf("%+v, want fetched and kept", got)
	}
}

// A contract with no end keeps the body, however old the notice is.
func TestAnOpenContractKeepsTheBody(t *testing.T) {
	n := eforms.Notice{
		Header:    eforms.Header{Root: "ContractAwardNotice"},
		AwardDate: ptr(day("2018-01-01")),
		Lots:      []eforms.Lot{{ID: "LOT-0001", Period: &eforms.ContractPeriod{Unlimited: true}}},
		Results:   []eforms.LotResult{{Lot: "LOT-0001", Status: "selec-w", Winners: []eforms.Winner{{Name: "Leverandør AS"}}}},
	}
	got, err := decide(n, true, fetched, 1, day("2030-01-01"))
	if err != nil {
		t.Fatalf("decide: %v", err)
	}
	if got.Basis != BasisOpen || got.Expired || !got.ExpiresAt.IsZero() {
		t.Errorf("%+v, want open and kept", got)
	}
}

func TestThePeriodIsBounded(t *testing.T) {
	for _, months := range []int{0, -1, MaxMonths + 1} {
		if _, err := Decide(nil, fetched, months, fetched); !errors.Is(err, ErrMonths) {
			t.Errorf("months %d: err %v, want ErrMonths", months, err)
		}
	}
	if _, err := Decide(nil, fetched, MaxMonths, fetched); err != nil {
		t.Errorf("months %d: %v, want accepted", MaxMonths, err)
	}
}

func ptr(t time.Time) *time.Time { return &t }
