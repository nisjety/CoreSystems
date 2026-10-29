package eforms_test

import (
	"reflect"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
)

// No notice of SDK 1.14 in the national flavour has been seen (parse.go says where it was looked for),
// so it is mapped by analogy and this test pins the analogy: such a notice reads field for field as
// its national-e twin of the same SDK, for each root. A real one, when it arrives, is what would show
// the analogy wrong.
func TestSDK114NationalReadsLikeItsNeighbours(t *testing.T) {
	const (
		national  = "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:national"
		nationalE = "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2026:eforms:national-e"
	)
	award := eformstest.Award{WinnerName: "Leverandør AS", WinnerOrg: "923609016", Tenders: 2, AwardDate: time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24}
	for name, build := range map[string]func(eformstest.Notice) []byte{
		"contract notice":          eformstest.ContractNotice,
		"award notice":             func(n eformstest.Notice) []byte { return eformstest.AwardWithResult(n, award) },
		"prior information notice": eformstest.PriorInformationNotice,
	} {
		base := eformstest.Notice{SDK: "1.14", Title: "Vedlikehold av ansattportal", Deadline: time.Date(2026, 12, 1, 12, 0, 0, 0, time.UTC), PlannedNotice: time.Date(2027, 2, 1, 0, 0, 0, 0, time.UTC)}
		twin := base
		base.CustomizationID, twin.CustomizationID = national, nationalE
		got, err := eforms.Parse(build(base))
		if err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		want, err := eforms.Parse(build(twin))
		if err != nil {
			t.Fatalf("%s, the national-e twin: %v", name, err)
		}
		if got.Flavour != eforms.FlavourNational || got.Title != "Vedlikehold av ansattportal" {
			t.Errorf("%s: flavour %q, title %q", name, got.Flavour, got.Title)
		}
		got.CustomizationID, want.CustomizationID = "", ""
		got.Flavour, want.Flavour = "", ""
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s reads differently from its national-e twin:\n got %+v\nwant %+v", name, got, want)
		}
	}
}
