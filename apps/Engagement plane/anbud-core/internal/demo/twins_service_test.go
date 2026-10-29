package demo

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
)

// twinCorpus holds one contract notice and one award notice twice each: from Doffin, with the
// Norwegian flavour, and from TED, plain EU eForms as TED republishes them, under the same eForms
// notice identifiers. TED's copies are the newest, so they are read first.
func twinCorpus() []rawstore.Document {
	cn := eformstest.Notice{ID: "00000000-0000-4000-8000-00000000c001", Title: "Drift av fagsystem", Deadline: time.Now().AddDate(0, 0, 20)}
	award := eformstest.Award{WinnerName: "Leverandør AS", WinnerOrg: "923609016", Tenders: 3, AwardDate: time.Now().AddDate(-1, 0, 0), DurationMonths: 24}
	can := eformstest.Notice{ID: "00000000-0000-4000-8000-00000000a001", Title: "Rammeavtale renhold"}
	plain := func(n eformstest.Notice) eformstest.Notice {
		n.CustomizationID = "eforms-sdk-1.14"
		return n
	}
	at := func(source, id string, body []byte) rawstore.Document {
		d := doc(id, body)
		d.Source = source
		return d
	}
	return []rawstore.Document{
		at("ted", "TED-700002-2026", eformstest.AwardWithResult(plain(can), award)),
		at("ted", "TED-700001-2026", eformstest.ContractNotice(plain(cn))),
		at("doffin", "2026-100002", eformstest.AwardWithResult(can, award)),
		at("doffin", "2026-100001", eformstest.ContractNotice(cn)),
	}
}

// The catalogue lists each notice once, under the Doffin id the app already knows, and counts it
// once in the total and the facets.
func TestTheCatalogueListsANoticeHeldFromDoffinAndTEDOnce(t *testing.T) {
	s := newService(t, &pagedNotices{fakeNotices{docs: twinCorpus()}}, nil)
	got, err := s.Catalogue(context.Background(), CatalogueRequest{})
	if err != nil {
		t.Fatalf("Catalogue: %v", err)
	}
	var ids []string
	for _, n := range got.Results {
		ids = append(ids, n.NoticeID)
	}
	if len(ids) != 2 || strings.Contains(strings.Join(ids, " "), "TED-") || got.TotalStored == nil || *got.TotalStored != 2 {
		t.Errorf("results %v, total %v; want the two Doffin ids only", ids, got.TotalStored)
	}
}

// The radar reads the award once: its winner is not counted a second time from the TED copy.
func TestTheRadarCountsAnAwardHeldTwiceOnce(t *testing.T) {
	req := decodeRadar(t, `{"profile":{"cpv":["72212000"]},"strictness":"wide","horizon_months":24}`)
	got, err := newService(t, &fakeNotices{docs: twinCorpus()}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if got.Scanned != 4 || got.Awards != 1 || len(got.Results) != 1 {
		t.Errorf("scanned %d, awards %d, results %d; want 4 read, 1 award, 1 contract", got.Scanned, got.Awards, len(got.Results))
	}
}
