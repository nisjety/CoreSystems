package bzp

import (
	"errors"
	"slices"
	"testing"
	"time"
)

// TestParseReadsAContractNoticeAsAContractNotice reads a captured contract notice: the identifiers, the title,
// the buyer, the closing date and the CPV codes are the record's own.
func TestParseReadsAContractNoticeAsAContractNotice(t *testing.T) {
	t.Parallel()
	n, err := Parse(fixtures(t)[0])
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Root != "ContractNotice" || n.NoticeTypeCode != "cn-standard" {
		t.Errorf("kind = %q, %q; want a contract notice of the standard type", n.Root, n.NoticeTypeCode)
	}
	if n.NoticeID != "2026/BZP 00505976/01" || n.ProcedureID != "ocds-148610-a67e6574-01a7-43a5-8367-6c420eee18da" {
		t.Errorf("ids = %q, %q", n.NoticeID, n.ProcedureID)
	}
	if n.Title != "Wymiana kotła gazowego" {
		t.Errorf("Title = %q", n.Title)
	}
	if !slices.Equal(n.BuyerNames, []string{"ZESPÓŁ SZKÓŁ CENTRUM KSZTAŁCENIA ROLNICZEGO IM. WINCENTEGO WITOSA W RÓŻAŃCU"}) {
		t.Errorf("BuyerNames = %q", n.BuyerNames)
	}
	wantDeadline := time.Date(2026, 11, 13, 9, 0, 0, 0, time.UTC)
	if n.Deadline == nil || !n.Deadline.Equal(wantDeadline) {
		t.Errorf("Deadline = %v, want %v", n.Deadline, wantDeadline)
	}
	if n.ProcurementType != "works" || !slices.Equal(n.CPVMain, []string{"45331110", "45333000", "45331100"}) {
		t.Errorf("procurement %q, CPV %q; want works and the eight digits of each code", n.ProcurementType, n.CPVMain)
	}
	if n.EstimatedValue != nil {
		t.Errorf("EstimatedValue = %+v; a board record states no value", n.EstimatedValue)
	}
}

func TestParseMapsTheOtherBoardKinds(t *testing.T) {
	t.Parallel()
	recs := fixtures(t)
	award, err := Parse(recs[1])
	if err != nil {
		t.Fatal(err)
	}
	if award.Root != "ContractAwardNotice" || award.NoticeTypeCode != "can-standard" || award.ProcurementType != "services" {
		t.Errorf("tender result = %q, %q, %q; want a contract award of the standard type, for services", award.Root, award.NoticeTypeCode, award.ProcurementType)
	}
	if award.Deadline != nil {
		t.Errorf("a tender result has a deadline %v", award.Deadline)
	}
	plan, err := Parse(recs[2])
	if err != nil {
		t.Fatal(err)
	}
	if plan.Root != "PriorInformationNotice" || plan.NoticeTypeCode != "pin-only" || plan.NoticeID != "2025/BZP 00610043/06/P" {
		t.Errorf("tender plan = %q, %q, %q; want a prior information notice keeping its /P", plan.Root, plan.NoticeTypeCode, plan.NoticeID)
	}
}

func TestParseRefusesWhatItCannotRead(t *testing.T) {
	t.Parallel()
	if _, err := Parse([]byte(`{"noticeNumber":"2026/BZP 00000001/01","noticeType":"ContractPerformingNotice"}`)); !errors.Is(err, ErrUnknownType) {
		t.Errorf("a performance notice: err = %v, want ErrUnknownType", err)
	}
	if _, err := Parse([]byte(`not json`)); err == nil {
		t.Error("a body that is not JSON was read")
	}
}

// TestCPVCodesAreTheirEightDigitsOnce: the board lists CPV codes with their labels and check digits; eForms
// writes the eight digits, once each.
func TestCPVCodesAreTheirEightDigitsOnce(t *testing.T) {
	t.Parallel()
	got := cpvCodes("45331110-0 (Instalowanie kotłów),45333000-0 (Roboty),45331110-0 (Instalowanie kotłów),38000000,not-a-code")
	if !slices.Equal(got, []string{"45331110", "45333000", "38000000"}) {
		t.Errorf("cpvCodes = %q", got)
	}
	if got := cpvCodes(""); len(got) != 0 {
		t.Errorf("an empty list gave %q", got)
	}
}
