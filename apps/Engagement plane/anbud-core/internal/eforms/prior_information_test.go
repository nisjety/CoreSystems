package eforms_test

import (
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
)

// Prior information notices read through the contract notice's paths: the same project, buyer and
// lots. Checked on 2026-10-28 against the 68 held in the local store (53 from Doffin in the eu and
// national-e flavours, 15 from TED in plain EU eForms; subtypes 1, 4, 5, 7, E1 and E2): all 68 read
// with a title, a buyer and a CPV code, and for the 15 from TED the buyer and the main CPV code were
// those TED's own search metadata names. What the contract notice's paths leave unread in them is the
// same as in contract notices, apart from the planned date, which is read as PlannedNoticeDate.
func TestPriorInformationNoticesReadLikeContractNotices(t *testing.T) {
	planned := time.Date(2027, 4, 13, 0, 0, 0, 0, time.UTC)
	for _, tc := range []struct{ sdk, customization string }{
		{"1.14", ""},
		{"1.13", ""},
		{"1.14", "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2026:eforms:national-e"},
		{"1.13", "eforms-sdk-1.13#extended#urn:fdc:anskaffelser.no:2026:eforms:national-e"},
		{"1.13", "eforms-sdk-1.13#extended#urn:fdc:anskaffelser.no:2023:eforms:national"},
		{"1.14", "eforms-sdk-1.14"},
		{"1.13", "eforms-sdk-1.13"},
	} {
		n, err := eforms.Parse(eformstest.PriorInformationNotice(eformstest.Notice{
			SDK: tc.sdk, CustomizationID: tc.customization, Title: "Ny barnehage", PlannedNotice: planned,
		}))
		if err != nil {
			t.Errorf("%s %q: %v", tc.sdk, tc.customization, err)
			continue
		}
		if n.Root != "PriorInformationNotice" || n.NoticeTypeCode != "pin-only" || n.Title != "Ny barnehage" ||
			len(n.BuyerNames) != 1 || len(n.CPVMain) != 1 || len(n.Lots) != 1 {
			t.Errorf("%s %q: read %q, type %q, %d buyers, cpv %v, %d lots", tc.sdk, tc.customization, n.Title, n.NoticeTypeCode, len(n.BuyerNames), n.CPVMain, len(n.Lots))
		}
		if n.PlannedNoticeDate == nil || !n.PlannedNoticeDate.Equal(planned) {
			t.Errorf("%s %q: planned notice date %v, want %s", tc.sdk, tc.customization, n.PlannedNoticeDate, planned.Format("2006-01-02"))
		}
	}
}

func TestAContractNoticeHasNoPlannedNoticeDate(t *testing.T) {
	n, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{}))
	if err != nil || n.PlannedNoticeDate != nil {
		t.Errorf("planned %v, err %v; want none", n.PlannedNoticeDate, err)
	}
}
