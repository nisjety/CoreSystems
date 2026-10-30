package fts

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"slices"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

// Parse reads a Find a Tender release package into the eForms model the readers share. A release's tag
// gives the kind of notice, as eForms' root does: a tender is a contract notice, an award a contract
// award notice and a planning notice a prior information notice. The eForms code of each kind is set
// as the notice type, so the readers label it the way they label an eForms notice of that kind.
//
// Only what the release states is set. Fields OCDS does not carry for a UK notice (the NUTS regions,
// the procedure type, the contracting authority's company number) stay empty; the readers show that
// as missing, not as a guess.
func Parse(body []byte) (eforms.Notice, error) {
	var pkg releasePackage
	if err := json.Unmarshal(body, &pkg); err != nil {
		return eforms.Notice{}, fmt.Errorf("fts: the body is not a release package: %w", err)
	}
	if len(pkg.Releases) == 0 {
		return eforms.Notice{}, ErrNoRelease
	}
	r := pkg.Releases[0]
	root, noticeType, ok := kindOf(r.Tag)
	if !ok {
		return eforms.Notice{}, fmt.Errorf("%w: %v", ErrUnknownTag, r.Tag)
	}

	n := eforms.Notice{Header: eforms.Header{
		Root:           root,
		NoticeID:       r.ID,
		ProcedureID:    r.OCID,
		NoticeTypeCode: noticeType,
	}}
	n.Title = r.Tender.Title
	n.Description = r.Tender.Description
	n.ProcurementType = procurementType(r.Tender.MainProcurementCategory)
	if r.Buyer != nil && r.Buyer.Name != "" {
		n.BuyerNames = []string{r.Buyer.Name}
	}
	if amount := r.Tender.Value; amount != nil {
		value := amount.Amount
		if value == 0 {
			value = amount.AmountGross
		}
		if value > 0 && amount.Currency != "" {
			n.EstimatedValue = &eforms.Amount{Minor: int64(math.Round(value * 100)), Currency: amount.Currency}
		}
	}
	if period := r.Tender.TenderPeriod; period != nil && period.EndDate != "" {
		if t, err := time.Parse(time.RFC3339, period.EndDate); err == nil {
			deadline := t.UTC()
			n.Deadline = &deadline
		}
	}
	n.CPVMain = cpvCodes(r.Tender)
	for _, l := range r.Tender.Lots {
		n.Lots = append(n.Lots, eforms.Lot{ID: l.ID, Title: l.Title})
	}
	for _, d := range r.Tender.Documents {
		if d.URL != "" {
			n.Documents = append(n.Documents, eforms.DocumentRef{URI: d.URL, Access: "unrestricted"})
		}
	}
	return n, nil
}

// The errors Parse returns for a body it cannot read as a notice.
var (
	ErrNoRelease  = errors.New("fts: the release package holds no release")
	ErrUnknownTag = errors.New("fts: the release's tag names no kind of notice the sync reads")
)

// releasePackage is the part of an OCDS release package the reader uses.
type releasePackage struct {
	Releases []release `json:"releases"`
}

type release struct {
	ID     string   `json:"id"`
	OCID   string   `json:"ocid"`
	Tag    []string `json:"tag"`
	Buyer  *party   `json:"buyer"`
	Tender tender   `json:"tender"`
}

type party struct {
	Name string `json:"name"`
}

type tender struct {
	Title                   string          `json:"title"`
	Description             string          `json:"description"`
	MainProcurementCategory string          `json:"mainProcurementCategory"`
	Value                   *money          `json:"value"`
	TenderPeriod            *period         `json:"tenderPeriod"`
	Classification          *classification `json:"classification"`
	Items                   []item          `json:"items"`
	Lots                    []lot           `json:"lots"`
	Documents               []document      `json:"documents"`
}

type money struct {
	Amount      float64 `json:"amount"`
	AmountGross float64 `json:"amountGross"`
	Currency    string  `json:"currency"`
}

type period struct {
	EndDate string `json:"endDate"`
}

type classification struct {
	Scheme string `json:"scheme"`
	ID     string `json:"id"`
}

type item struct {
	Classification *classification `json:"classification"`
}

type lot struct {
	ID    string `json:"id"`
	Title string `json:"title"`
}

type document struct {
	URL string `json:"url"`
}

// kindOf maps a release's tags to the eForms root and notice type of its kind. A release carries one
// tag, which is the kind it was published as; a tag the sync does not read is refused.
func kindOf(tags []string) (root, noticeType string, ok bool) {
	switch {
	case slices.Contains(tags, "tender"):
		return "ContractNotice", "cn-standard", true
	case slices.Contains(tags, "award"):
		return "ContractAwardNotice", "can-standard", true
	case slices.Contains(tags, "planning"):
		return "PriorInformationNotice", "pin-only", true
	}
	return "", "", false
}

// procurementType maps OCDS's main procurement category to the eForms code of the same meaning.
func procurementType(category string) string {
	switch category {
	case "services":
		return "services"
	case "goods":
		return "supplies"
	case "works":
		return "works"
	}
	return ""
}

// cpvCodes are the CPV codes of the release, the tender's classification first and then its items', without
// the check digit (72000000-5 is 72000000, as eForms writes it), each once and in order.
func cpvCodes(t tender) []string {
	var codes []string
	add := func(c *classification) {
		if c == nil || !strings.EqualFold(c.Scheme, "CPV") {
			return
		}
		code, _, _ := strings.Cut(c.ID, "-")
		if code != "" && !slices.Contains(codes, code) {
			codes = append(codes, code)
		}
	}
	add(t.Classification)
	for _, it := range t.Items {
		add(it.Classification)
	}
	return codes
}
