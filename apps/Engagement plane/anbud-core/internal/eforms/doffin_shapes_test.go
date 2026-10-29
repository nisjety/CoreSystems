package eforms_test

import (
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
)

// Every shape Doffin served in the held corpus reads. On 2026-10-28, 78 of the 1 056 held notices were
// refused, all from Doffin, in six shapes; each is mapped to the extractor of its kind, and checked
// against Doffin's own search metadata, which a parse cannot have influenced: for the 54 that had it,
// the title and the buyer matched in 54, the tender deadline of the 29 contract notices in 29, and
// the winners of the 17 award notices that name any in 17. (Doffin's search gives an award notice's
// contract end as its "deadline"; that is not a tender deadline and was not compared.) The six
// national-flavour prior information notices found on Doffin the same day read too, with the title and
// the buyer of Doffin's metadata.
func TestEveryHeldDoffinShapeReads(t *testing.T) {
	const (
		nationalE = "#extended#urn:fdc:anskaffelser.no:2026:eforms:national-e"
		national  = "#extended#urn:fdc:anskaffelser.no:2023:eforms:national"
		eu        = "#extended#urn:fdc:anskaffelser.no:2023:eforms:eu"
	)
	award := eformstest.Award{WinnerName: "Leverandør AS", WinnerOrg: "923609016", Tenders: 2, AwardDate: time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24}
	withResult := func(n eformstest.Notice) []byte { return eformstest.AwardWithResult(n, award) }
	for _, tc := range []struct {
		name   string
		build  func(eformstest.Notice) []byte
		sdk    string
		suffix string
		award  bool
	}{
		{"contract notice 1.13 national-e", eformstest.ContractNotice, "1.13", nationalE, false},
		{"contract notice 1.13 national", eformstest.ContractNotice, "1.13", national, false},
		{"award notice 1.13 eu", withResult, "1.13", eu, true},
		{"award notice 1.14 national-e", withResult, "1.14", nationalE, true},
		{"award notice 1.13 national-e", withResult, "1.13", nationalE, true},
		{"award notice 1.13 national", withResult, "1.13", national, true},
	} {
		n, err := eforms.Parse(tc.build(eformstest.Notice{SDK: tc.sdk, CustomizationID: "eforms-sdk-" + tc.sdk + tc.suffix, Title: "Drift av fagsystem"}))
		if err != nil {
			t.Errorf("%s: %v", tc.name, err)
			continue
		}
		if n.Title != "Drift av fagsystem" || len(n.BuyerNames) != 1 || len(n.CPVMain) != 1 {
			t.Errorf("%s: read %q, %d buyers, cpv %v", tc.name, n.Title, len(n.BuyerNames), n.CPVMain)
		}
		if tc.award && (len(n.Results) != 1 || len(n.Results[0].Winners) != 1 || n.Results[0].Winners[0].Name != "Leverandør AS") {
			t.Errorf("%s: results %+v, want the fixture's winner", tc.name, n.Results)
		}
	}
}
