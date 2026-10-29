package eforms_test

import (
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
)

// TED publishes every notice as plain EU eForms, Norwegian ones included: the CustomizationID is the
// SDK and nothing else. Such a notice reads exactly as its Norwegian eu flavour does, field for field,
// because the field maps read the same UBL paths. Checked on live TED notices on 2026-10-28: 41 of 50
// Nordic notices read (contract notices of 1.13 and 1.14, award notices of 1.13 and 1.14; the prior
// information notices among them read too, see prior_information_test.go), and the winners of all 23
// award notices were the tenderers TED's own search metadata names.
func TestPlainTEDNoticesReadLikeTheirNorwegianFlavour(t *testing.T) {
	deadline := time.Date(2026, 11, 30, 12, 0, 0, 0, time.UTC)
	for _, tc := range []struct {
		name  string
		build func(eformstest.Notice) []byte
		sdk   string
	}{
		{"contract notice 1.14", eformstest.ContractNotice, "1.14"},
		{"contract notice 1.13", eformstest.ContractNotice, "1.13"},
		{"award notice 1.14", awardFixture, "1.14"},
		{"award notice 1.13", awardFixture, "1.13"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			base := eformstest.Notice{SDK: tc.sdk, Title: "Rammeavtale", CPV: "72212000", Deadline: deadline}
			plain := base
			plain.CustomizationID = "eforms-sdk-" + tc.sdk

			want, wantErr := eforms.Parse(tc.build(base))
			if wantErr != nil && tc.sdk == "1.14" {
				t.Fatalf("the Norwegian flavour: %v", wantErr)
			}
			got, err := eforms.Parse(tc.build(plain))
			if err != nil {
				t.Fatalf("the plain notice: %v", err)
			}
			if got.Flavour != "" || got.CustomizationID != "eforms-sdk-"+tc.sdk {
				t.Errorf("header flavour %q, customization %q; want empty and the plain SDK", got.Flavour, got.CustomizationID)
			}
			if got.Title != "Rammeavtale" || len(got.CPVMain) != 1 || len(got.Lots) != 1 || len(got.BuyerNames) != 1 {
				t.Errorf("read %q, cpv %v, %d lots, %d buyers; want the fixture's", got.Title, got.CPVMain, len(got.Lots), len(got.BuyerNames))
			}
			// The Norwegian flavour has no 1.13 award map, so only the 1.14 shapes and the 1.13
			// contract notice have a twin to compare with.
			if wantErr != nil {
				return
			}
			got.CustomizationID, want.CustomizationID = "", ""
			got.Flavour, want.Flavour = "", ""
			if !reflect.DeepEqual(got, want) {
				t.Errorf("the plain notice reads differently from its Norwegian flavour:\n got %+v\nwant %+v", got, want)
			}
		})
	}
}

func awardFixture(n eformstest.Notice) []byte {
	return eformstest.AwardWithResult(n, eformstest.Award{
		WinnerName: "Leverandør AB", WinnerOrg: "556000-0000", Tenders: 2, AwardDate: time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24,
	})
}

func TestAPlainAwardNamesItsWinner(t *testing.T) {
	n, err := eforms.Parse(awardFixture(eformstest.Notice{SDK: "1.13", CustomizationID: "eforms-sdk-1.13"}))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if len(n.Results) != 1 || len(n.Results[0].Winners) != 1 || n.Results[0].Winners[0].Name != "Leverandør AB" || n.AwardDate == nil {
		t.Errorf("results %+v, award date %v; want one result won by the fixture's winner", n.Results, n.AwardDate)
	}
}

// Only an exactly plain CustomizationID counts as plain EU. Another country's extension leaves the
// flavour empty too, and is still refused: its paths are not known to this code.
func TestAForeignExtensionIsStillRefused(t *testing.T) {
	for _, c := range []string{
		"eforms-sdk-1.14#extended#urn:fdc:example.eu:2024:eforms:national",
		"eforms-sdk-1.13-extra",
	} {
		_, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{CustomizationID: c}))
		if !errors.Is(err, eforms.ErrNoFieldMap) {
			t.Errorf("%q: err %v, want ErrNoFieldMap", c, err)
		}
	}
}
