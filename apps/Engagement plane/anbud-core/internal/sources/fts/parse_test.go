package fts

import (
	"errors"
	"os"
	"slices"
	"testing"
	"time"
)

// TestParseReadsATenderAsAContractNotice reads a captured Find a Tender notice into the eForms model: the
// identifiers, the title, the buyer, the value in minor units and the closing date are the notice's own.
func TestParseReadsATenderAsAContractNotice(t *testing.T) {
	t.Parallel()
	body, err := os.ReadFile("testdata/notice_101800.json")
	if err != nil {
		t.Fatal(err)
	}
	n, err := Parse(body)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Root != "ContractNotice" || n.NoticeTypeCode != "cn-standard" {
		t.Errorf("kind = %q, %q; want a contract notice of the standard type", n.Root, n.NoticeTypeCode)
	}
	if n.NoticeID != "101800-2026" || n.ProcedureID != "ocds-h6vhtk-0791ce" {
		t.Errorf("ids = %q, %q; want the notice number and the contract's ocid", n.NoticeID, n.ProcedureID)
	}
	if n.Title != "Catering Service (non staff)for New Wave Federation Primary Schools" {
		t.Errorf("Title = %q", n.Title)
	}
	if !slices.Equal(n.BuyerNames, []string{"New Wave Federation"}) {
		t.Errorf("BuyerNames = %q", n.BuyerNames)
	}
	if n.EstimatedValue == nil || n.EstimatedValue.Minor != 240000000 || n.EstimatedValue.Currency != "GBP" {
		t.Errorf("EstimatedValue = %+v, want 2 400 000.00 GBP in minor units", n.EstimatedValue)
	}
	wantDeadline := time.Date(2026, 11, 23, 12, 0, 0, 0, time.UTC)
	if n.Deadline == nil || !n.Deadline.Equal(wantDeadline) {
		t.Errorf("Deadline = %v, want %v", n.Deadline, wantDeadline)
	}
	if n.ProcurementType != "services" || len(n.Lots) != 1 || len(n.Documents) == 0 {
		t.Errorf("procurement %q, lots %d, documents %d; want services, one lot and the documents", n.ProcurementType, len(n.Lots), len(n.Documents))
	}
}

func TestParseMapsEachKindOfRelease(t *testing.T) {
	t.Parallel()
	for tag, want := range map[string]struct{ root, noticeType string }{
		"award":    {"ContractAwardNotice", "can-standard"},
		"planning": {"PriorInformationNotice", "pin-only"},
	} {
		body := []byte(`{"releases":[{"id":"101800-2026","ocid":"ocds-h6vhtk-000001","tag":["` + tag + `"],"tender":{"title":"T"}}]}`)
		n, err := Parse(body)
		if err != nil {
			t.Fatalf("%s: %v", tag, err)
		}
		if n.Root != want.root || n.NoticeTypeCode != want.noticeType {
			t.Errorf("%s: kind = %q, %q; want %q, %q", tag, n.Root, n.NoticeTypeCode, want.root, want.noticeType)
		}
	}
}

func TestParseRefusesWhatItCannotRead(t *testing.T) {
	t.Parallel()
	if _, err := Parse([]byte(`{"releases":[{"id":"1-2026","tag":["contract"],"tender":{}}]}`)); !errors.Is(err, ErrUnknownTag) {
		t.Errorf("an unknown tag: err = %v, want ErrUnknownTag", err)
	}
	if _, err := Parse([]byte(`{"releases":[]}`)); !errors.Is(err, ErrNoRelease) {
		t.Errorf("no release: err = %v, want ErrNoRelease", err)
	}
	if _, err := Parse([]byte(`not json`)); err == nil {
		t.Error("a body that is not JSON was read")
	}
}

// TestParseTakesCPVWithoutItsCheckDigit: eForms writes a CPV code as its eight digits, so the reader does too,
// once each, the tender's classification first.
func TestParseTakesCPVWithoutItsCheckDigit(t *testing.T) {
	t.Parallel()
	body := []byte(`{"releases":[{"id":"101800-2026","tag":["tender"],"tender":{
		"classification":{"scheme":"CPV","id":"72212000-0"},
		"items":[{"classification":{"scheme":"CPV","id":"72212000-0"}},{"classification":{"scheme":"CPV","id":"79340000-9"}},{"classification":{"scheme":"Other","id":"X1"}}]}}]}`)
	n, err := Parse(body)
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(n.CPVMain, []string{"72212000", "79340000"}) {
		t.Errorf("CPVMain = %q, want the eight digits of each CPV code, once each", n.CPVMain)
	}
}
