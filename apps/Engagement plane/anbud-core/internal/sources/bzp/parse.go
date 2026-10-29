package bzp

import (
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"slices"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

// Parse reads a BZP board record into the eForms model the readers share. The board's notice type gives the
// kind of notice, as eForms' root does: a contract notice is a contract notice, a tender result a contract
// award notice and a tender plan a prior information notice. The eForms code of each kind is set as the
// notice type, so the readers label it the way they label an eForms notice of that kind.
//
// A board record carries the title, the buyer, the procedure's OCDS identifier, the closing date and the
// CPV codes, and no contract value, so the value and the descriptions a record does not state stay empty.
func Parse(body []byte) (eforms.Notice, error) {
	var r notice
	if err := json.Unmarshal(body, &r); err != nil {
		return eforms.Notice{}, fmt.Errorf("bzp: the body is not a board record: %w", err)
	}
	root, noticeType, ok := kindOf(r.NoticeType)
	if !ok {
		return eforms.Notice{}, fmt.Errorf("%w: %q", ErrUnknownType, r.NoticeType)
	}

	n := eforms.Notice{Header: eforms.Header{
		Root:           root,
		NoticeID:       r.NoticeNumber,
		ProcedureID:    r.TenderID,
		NoticeTypeCode: noticeType,
	}}
	n.Title = r.OrderObject
	if r.OrganizationName != "" {
		n.BuyerNames = []string{r.OrganizationName}
	}
	n.ProcurementType = procurementType(r.OrderType)
	if deadline, err := time.Parse(time.RFC3339Nano, r.SubmittingOffersDate); err == nil {
		utc := deadline.UTC()
		n.Deadline = &utc
	}
	n.CPVMain = cpvCodes(r.CPVCode)
	return n, nil
}

// ErrUnknownType refuses a board record of a notice type the sync does not read.
var ErrUnknownType = errors.New("bzp: the notice type is not one the sync reads")

// notice is the part of a board record the reader uses.
type notice struct {
	NoticeNumber         string `json:"noticeNumber"`
	NoticeType           string `json:"noticeType"`
	OrderType            string `json:"orderType"`
	OrderObject          string `json:"orderObject"`
	CPVCode              string `json:"cpvCode"`
	SubmittingOffersDate string `json:"submittingOffersDate"`
	OrganizationName     string `json:"organizationName"`
	TenderID             string `json:"tenderId"`
}

// kindOf maps a board notice type to the eForms root and notice type of its kind. The types the sync reads are
// the three that have a kind here; any other is left out.
func kindOf(noticeType string) (root, code string, ok bool) {
	switch noticeType {
	case "ContractNotice":
		return "ContractNotice", "cn-standard", true
	case "TenderResultNotice":
		return "ContractAwardNotice", "can-standard", true
	case "TenderPlanNotice":
		return "PriorInformationNotice", "pin-only", true
	}
	return "", "", false
}

// procurementType maps the board's order type to the eForms procurement type of the same meaning.
func procurementType(orderType string) string {
	switch orderType {
	case "Works":
		return "works"
	case "Services":
		return "services"
	case "Supplies", "Delivery":
		return "supplies"
	}
	return ""
}

var cpvPattern = regexp.MustCompile(`^[0-9]{8}$`)

// cpvCodes are the CPV codes of a record, which the board lists with their labels ("45331110-0 (Instalowanie
// kotłów),45333000-0 (...)"). Each code is its eight digits, without the check digit or the label, as eForms
// writes it, once each and in order.
func cpvCodes(list string) []string {
	var codes []string
	for _, item := range strings.Split(list, ",") {
		code, _, _ := strings.Cut(strings.TrimSpace(item), " ")
		code, _, _ = strings.Cut(code, "-")
		if cpvPattern.MatchString(code) && !slices.Contains(codes, code) {
			codes = append(codes, code)
		}
	}
	return codes
}
