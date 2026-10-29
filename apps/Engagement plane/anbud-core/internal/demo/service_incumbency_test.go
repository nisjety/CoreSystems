package demo

import (
	"context"
	"encoding/json"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
)

// fakeRegister answers a lookup from what it holds, and ErrNotFound for anything else.
type fakeRegister struct {
	orgs map[string]orgcore.Organisation

	mu    sync.Mutex
	calls []string
}

func (f *fakeRegister) Lookup(_ context.Context, number string) (orgcore.Organisation, error) {
	f.mu.Lock()
	f.calls = append(f.calls, number)
	f.mu.Unlock()
	if o, ok := f.orgs[number]; ok {
		return o, nil
	}
	return orgcore.Organisation{}, orgcore.ErrNotFound
}

var register = map[string]orgcore.Organisation{
	"111111111": {OrgNumber: "111111111", Name: "Fiktiv Leverandør AS", OrgForm: "AS"},
	"333333333": {OrgNumber: "333333333", Name: "Ola Fiktivsen", OrgForm: "ENK"},
}

const soleTraderName = "Ola Fiktivsen"

// otherBuyersWinner won only the other buyer's award: the register is never asked about it.
const otherBuyersWinner = "444444444"

func awardDoc(id, procedure, buyer string, a eformstest.Award) rawstore.Document {
	return doc(id, eformstest.AwardWithResult(eformstest.Notice{
		ID: "00000000-0000-4000-8000-0000000" + strings.ReplaceAll(id, "-", "")[:5], ProcedureID: procedure, CPV: "72212000", BuyerOrg: buyer,
	}, a))
}

// history is the tender being audited and three award notices: two from its buyer (one won by a
// limited company, one by a sole trader) and one from another buyer.
func history() []rawstore.Document {
	return []rawstore.Document{
		auditNotice(eformstest.Notice{Title: "Drift av fagsystem", CPV: "72212000", NUTS: "NO081"}),
		awardDoc("2024-200001", "00000000-0000-4000-8000-0000000000a1", "",
			eformstest.Award{WinnerName: "Fiktiv Leverandør AS", WinnerOrg: "111111111", Tenders: 4, AwardDate: time.Date(2024, 11, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24}),
		awardDoc("2023-200002", "00000000-0000-4000-8000-0000000000a2", "",
			eformstest.Award{WinnerName: soleTraderName, WinnerOrg: "333333333", Tenders: 2, AwardDate: time.Date(2023, 5, 1, 0, 0, 0, 0, time.UTC)}),
		awardDoc("2024-200003", "00000000-0000-4000-8000-0000000000a3", "922222222",
			eformstest.Award{WinnerName: "Fjern Fiktiv AS", WinnerOrg: otherBuyersWinner, AwardDate: time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC)}),
	}
}

func TestAuditShowsTheBuyersPastAwards(t *testing.T) {
	t.Parallel()

	reg := &fakeRegister{orgs: register}
	got, err := newService(t, &fakeNotices{docs: history()}, reg).Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	st := got.IncumbencyStatus
	if st == nil || !st.Checked || st.Awards != 3 || st.Winners != "checked" {
		t.Fatalf("incumbency status = %+v", st)
	}
	p := got.Incumbency
	if p == nil || p.Related != 2 || len(p.Awards) != 2 {
		t.Fatalf("panel = %+v, want the buyer's two awards and not the other buyer's", p)
	}
	in := p.Incumbent
	if in == nil || in.NoticeID != "2024-200001" || in.Relation != "same_cpv" || in.AwardDate != "2024-11-01" ||
		in.ContractEnd != "2026-11-01" || in.EndBasis != "awarded+duration" {
		t.Errorf("incumbent = %+v", in)
	}
	if w := in.Winners; len(w) != 1 || w[0].Name != "Fiktiv Leverandør AS" || w[0].OrgNumber != "111111111" || w[0].Status != "organisation" {
		t.Errorf("incumbent's winners = %+v", w)
	}
	if w := p.Awards[1].Winners; len(w) != 1 || w[0].Name != "" || w[0].OrgNumber != "" || w[0].Status != "sole_trader" {
		t.Errorf("a sole trader is shown: %+v", w)
	}
	if len(p.TopWinners) != 1 || p.TopWinners[0].Wins != 1 || p.WithheldWinners != 1 {
		t.Errorf("top winners %+v withheld %d", p.TopWinners, p.WithheldWinners)
	}
	if p.MedianTendersReceived == nil || *p.MedianTendersReceived != 3 {
		t.Errorf("median tenders = %v, want 3 of 2 and 4", p.MedianTendersReceived)
	}

	out, _ := json.Marshal(got)
	for _, personal := range append([]string{soleTraderName, "333333333"}, eformstest.PersonalMarkers...) {
		if strings.Contains(string(out), personal) {
			t.Errorf("the audit sheet carries %q", personal)
		}
	}
	reg.mu.Lock()
	defer reg.mu.Unlock()
	for _, n := range reg.calls {
		if n == otherBuyersWinner {
			t.Errorf("the register was asked about a winner of another buyer's award")
		}
	}
}

// Without the register, no winner can be checked, so none is named and none is ranked. The rest of
// the panel (the awards, their days, contract ends and tender counts) stands.
func TestAuditWithoutTheRegisterNamesNoWinner(t *testing.T) {
	t.Parallel()

	got, err := newService(t, &fakeNotices{docs: history()}, nil).Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if got.IncumbencyStatus == nil || got.IncumbencyStatus.Winners != "not_configured" {
		t.Errorf("status = %+v", got.IncumbencyStatus)
	}
	p := got.Incumbency
	if p == nil || p.Related != 2 || len(p.TopWinners) != 0 || p.WithheldWinners != 2 {
		t.Fatalf("panel = %+v", p)
	}
	for _, a := range p.Awards {
		for _, w := range a.Winners {
			if w.Name != "" || w.OrgNumber != "" || w.Status != "unverified" {
				t.Errorf("an unchecked winner is shown: %+v", w)
			}
		}
	}
	if p.Incumbent == nil || p.Incumbent.ContractEnd != "2026-11-01" {
		t.Errorf("the incumbent's contract end is lost without the register: %+v", p.Incumbent)
	}
}

// A register that fails for one winner leaves that winner unverified and names the rest.
func TestAuditWhenTheRegisterFailsForAWinner(t *testing.T) {
	t.Parallel()

	reg := &fakeRegister{orgs: map[string]orgcore.Organisation{"111111111": register["111111111"]}}
	got, err := newService(t, &fakeNotices{docs: history()}, reg).Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if got.IncumbencyStatus.Winners != "partial" {
		t.Errorf("winners status = %q, want partial", got.IncumbencyStatus.Winners)
	}
	if w := got.Incumbency.Awards[1].Winners[0]; w.Status != "unverified" || w.Name != "" {
		t.Errorf("a winner the register did not answer for is shown: %+v", w)
	}
}

// The radar counts what it read, and lists nothing that ended before today or states no end.
func TestReTendersListsContractsEndingWithinTheHorizon(t *testing.T) {
	t.Parallel()

	docs := append(history(),
		awardDoc("2020-200004", "00000000-0000-4000-8000-0000000000a4", "",
			eformstest.Award{WinnerName: "Fiktiv Leverandør AS", WinnerOrg: "111111111", AwardDate: time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24}),
	)
	req, errs := DecodeReTenderRequest([]byte(`{"profile":{"cpv":["72000000"]},"horizon_months":12}`))
	if len(errs) != 0 {
		t.Fatalf("decode: %+v", errs)
	}
	got, err := newService(t, &fakeNotices{docs: docs}, &fakeRegister{orgs: register}).ReTenders(context.Background(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if got.Window.From != "2026-11-10" || got.Window.To != "2027-11-10" || got.Scanned != 5 || got.Awards != 4 || got.Winners != "checked" {
		t.Errorf("header = %+v", got)
	}
	// The award of 2024 ended on 2026-11-01, nine days before now; the others state no duration or
	// ended long ago. None is ending in the next twelve months.
	if len(got.Results) != 0 {
		t.Errorf("results = %+v, want none", got.Results)
	}
}

// A contract that ends inside the horizon is on the radar with its buyer and winner.
func TestReTendersShowsTheBuyerAndTheWinner(t *testing.T) {
	t.Parallel()

	docs := []rawstore.Document{awardDoc("2025-200005", "00000000-0000-4000-8000-0000000000a5", "",
		eformstest.Award{WinnerName: "Fiktiv Leverandør AS", WinnerOrg: "111111111", AwardDate: time.Date(2025, 6, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24})}
	req, _ := DecodeReTenderRequest([]byte(`{"horizon_months":12}`))
	got, err := newService(t, &fakeNotices{docs: docs}, &fakeRegister{orgs: register}).ReTenders(context.Background(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if len(got.Results) != 1 {
		t.Fatalf("results = %+v", got.Results)
	}
	r := got.Results[0]
	if r.ContractEnd != "2027-06-01" || r.Buyer != "Eksempelkommune" || len(r.Winners) != 1 || r.Winners[0].Name != "Fiktiv Leverandør AS" {
		t.Errorf("entry = %+v", r)
	}
}

func TestDecodeReTenderRequest(t *testing.T) {
	t.Parallel()

	r, errs := DecodeReTenderRequest([]byte(`{}`))
	if len(errs) != 0 || r.HorizonMonths != DefaultHorizonMonths || r.Scan != MaxScan {
		t.Errorf("defaults = %+v %+v", r, errs)
	}
	for _, body := range []string{`{"horizon_months":0}`, `{"horizon_months":37}`, `{"scan":0}`, `{"unknown":1}`, `{"profile":{"cpv":["not a code"]}}`} {
		if _, errs := DecodeReTenderRequest([]byte(body)); len(errs) == 0 {
			t.Errorf("%s was accepted", body)
		}
	}
}
