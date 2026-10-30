package demo

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
)

func TestSourceDetailsExposeFactsWithoutRawWinnerNames(t *testing.T) {
	zero, weight := 0, 40
	signed := time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
	n := eforms.Notice{Requirements: []eforms.Requirement{{Type: "exg-fraud", Text: "Source exclusion"}}, Lots: []eforms.Lot{{ID: "LOT-1", Renewals: &zero, AwardCriteria: []eforms.AwardCriterion{{Type: "quality", Weight: &weight}}}}, Results: []eforms.LotResult{{Lot: "LOT-1", TendersReceived: &zero, ContractSigned: &signed, Winners: []eforms.Winner{{Name: "UNVERIFIED WINNER"}}}}, Change: &eforms.ChangeInfo{Reason: "Source amendment", Replaces: "original-01", Sections: []string{"LOT-1"}}}
	c := candidate{Candidate: match.Candidate{Notice: n}}
	view := newAudit(rawstore.Document{}, c, match.Assessment{}, match.Evidence{}, signed).Notice
	if len(view.ExclusionCriteria) != 1 || len(view.Awards) != 1 || view.Change.Reason != "Source amendment" || *view.Lots[0].Renewals != 0 {
		t.Fatalf("missing source facts: %+v", view)
	}
	body, err := json.Marshal(view)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(body), "UNVERIFIED WINNER") || !strings.Contains(string(body), `"weight_percent":40`) || !strings.Contains(string(body), `"tenders_received":0`) {
		t.Fatalf("unsafe or incomplete source projection: %s", body)
	}
}
