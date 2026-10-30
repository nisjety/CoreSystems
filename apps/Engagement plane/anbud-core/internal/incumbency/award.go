// Package incumbency reads award notices for what they say about a market: who a buyer has awarded
// before, how many tenders it got, and when those contracts end and so are likely to be tendered
// again. It is pure: it reads parsed notices and keeps nothing.
//
// Winners are named only when the register says they are organisations that are not sole traders.
// A sole trader's name is a person's name, and naming people across many notices, or ranking them,
// is profiling a person; a winner that cannot be checked is treated the same way. This rule is a
// proposal awaiting a compliance check (docs/PERSONAL-DATA.md).
package incumbency

import (
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
)

// winnerChosen is the winner-selection code of a lot result whose winner was chosen.
const winnerChosen = "selec-w"

// Award is one awarded lot of a result notice. It holds the winners as the notice names them, so it
// is never shown as it is: Build and ReTenders decide what of a winner may be shown.
type Award struct {
	NoticeID        string // the Doffin id the notice is stored under
	ProcedureID     string
	Lot             string
	Title           string
	LotTitle        string
	BuyerNames      []string
	BuyerOrgNumbers []string
	CPV             []string // the lot's main codes, or the notice's when the lot states none
	NUTS            []string
	AwardDate       *time.Time
	Signed          *time.Time
	Period          *eforms.ContractPeriod
	Renewals        *int
	TendersReceived *int
	Value           *eforms.Amount
	Winners         []eforms.Winner
}

// FromNotice returns the awarded lots of a result notice: one Award for each lot result with a winner
// chosen and at least one winner the parser could follow. Any other notice gives none.
func FromNotice(noticeID string, n eforms.Notice) []Award {
	if n.Root != "ContractAwardNotice" {
		return nil
	}
	lots := make(map[string]eforms.Lot, len(n.Lots))
	for _, l := range n.Lots {
		lots[l.ID] = l
	}
	var out []Award
	for _, r := range n.Results {
		if r.Status != winnerChosen || len(r.Winners) == 0 {
			continue
		}
		lot := lots[r.Lot]
		cpv := codes(lot.CPVMain)
		if len(cpv) == 0 {
			cpv = codes(n.CPVMain)
		}
		out = append(out, Award{
			NoticeID: noticeID, ProcedureID: n.ProcedureID, Lot: r.Lot, Title: n.Title, LotTitle: lot.Title,
			BuyerNames: n.BuyerNames, BuyerOrgNumbers: n.BuyerOrgNumbers, CPV: cpv, NUTS: regions(n.NUTS),
			AwardDate: n.AwardDate, Signed: r.ContractSigned, Period: lot.Period, Renewals: lot.Renewals,
			TendersReceived: r.TendersReceived, Value: r.Value, Winners: r.Winners,
		})
	}
	return out
}

func codes(raw []string) []string {
	var out []string
	for _, c := range raw {
		if code, err := normalize.CPV(c); err == nil {
			out = append(out, code)
		}
	}
	return out
}

func regions(raw []string) []string {
	var out []string
	for _, c := range raw {
		if code, err := normalize.NUTS(c); err == nil {
			out = append(out, code)
		}
	}
	return out
}

// EndBasis says where an end date comes from.
type EndBasis string

const (
	EndStated     EndBasis = "stated"           // the notice states the end
	EndFromStart  EndBasis = "start+duration"   // the stated start plus the stated duration
	EndFromSigned EndBasis = "signed+duration"  // the day the contract was signed plus the duration
	EndFromAward  EndBasis = "awarded+duration" // the award day plus the duration
	EndOpen       EndBasis = "open"             // the notice says the contract has no end
	EndUnknown    EndBasis = "unknown"
)

// End is when a contract's base period ends. At is nil when it is open or unknown. An end that is
// not stated is an estimate, and Basis says from what; renewals, when the buyer may make any, can
// push it later, and are not added.
type End struct {
	At    *time.Time
	Basis EndBasis
}

// End says when the award's contract ends: the stated end; or, given a duration, the stated start,
// the signing day or the award day plus the duration, in that order; open when the notice says so;
// otherwise unknown.
func (a Award) End() End {
	p := a.Period
	if p == nil {
		return End{Basis: EndUnknown}
	}
	if p.End != nil {
		return End{At: p.End, Basis: EndStated}
	}
	if p.Duration != nil {
		for _, from := range []struct {
			at    *time.Time
			basis EndBasis
		}{{p.Start, EndFromStart}, {a.Signed, EndFromSigned}, {a.AwardDate, EndFromAward}} {
			if from.at != nil {
				end := add(*from.at, *p.Duration)
				return End{At: &end, Basis: from.basis}
			}
		}
	}
	if p.Unlimited {
		return End{Basis: EndOpen}
	}
	return End{Basis: EndUnknown}
}

func add(t time.Time, d eforms.Duration) time.Time {
	switch d.Unit {
	case "DAY":
		return t.AddDate(0, 0, d.Value)
	case "WEEK":
		return t.AddDate(0, 0, 7*d.Value)
	case "MONTH":
		return t.AddDate(0, d.Value, 0)
	default: // YEAR: the parser admits no other unit
		return t.AddDate(d.Value, 0, 0)
	}
}

// when is the day an award took effect, for ordering: the signing day, else the award day.
func (a Award) when() *time.Time {
	if a.Signed != nil {
		return a.Signed
	}
	return a.AwardDate
}

// key identifies an awarded lot, so that the same notice read twice is counted once.
func (a Award) key() string { return a.NoticeID + "\x00" + a.Lot }
