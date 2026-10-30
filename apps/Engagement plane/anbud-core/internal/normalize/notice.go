package normalize

import (
	"errors"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

// The rules a Change can name.
const (
	RuleCPVCheckDigit = "cpv: check digit dropped"
	RuleUpperCase     = "upper case"
	RuleOrgNumberForm = "org number: spaces, dots, NO and MVA removed"
)

// Change records one correction: which field, the value before and after, and the rule applied.
type Change struct {
	Field, From, To, Rule string
}

// DeadlineKind says which step a deadline is for.
type DeadlineKind string

const (
	// DeadlineTender is the deadline for tenders.
	DeadlineTender DeadlineKind = "tender"
	// DeadlineParticipation is the deadline for requests to participate, which a restricted
	// procedure states instead: tenders are invited only from those who asked in time.
	DeadlineParticipation DeadlineKind = "participation"
)

// Deadline is the deadline that binds a company that has not yet acted, for the notice as a whole
// (Lot is "") or for one lot.
type Deadline struct {
	Lot  string
	Kind DeadlineKind
	At   time.Time
}

// Result is a notice's normalised codes, identities and deadlines. It is a new value: the notice
// it came from is not changed.
type Result struct {
	CPVMain          []string // the notice's main codes
	CPV              []string // every code the notice and its lots state, each once, main codes first
	NUTS             []string
	BuyerOrgNumbers  []string
	WinnerOrgNumbers []string
	// Deadlines holds one deadline for the notice and one per lot, where it states any. When a
	// scope states a deadline for requests to participate, that is the one that binds, because a
	// company that has not asked to take part cannot tender; otherwise it is the tender deadline.
	Deadlines []Deadline
	// EarliestDeadline is the earliest of Deadlines, or nil when there are none. Without a clock
	// it cannot say which are still open: the caller does.
	EarliestDeadline *time.Time

	Changes  []Change
	Problems []string
}

// Notice normalises a parsed notice. A value that cannot be normalised is left out, and a problem
// names the field and the reason without the value.
func Notice(n eforms.Notice) Result {
	var r Result
	// The main codes are normalised twice: once on their own, with nothing recorded, and once as
	// part of every code, where their changes and problems are recorded.
	var unrecorded Result
	r.CPVMain = dedupe(unrecorded.cpvs(n.CPVMain, nil))
	seen := map[string]bool{}
	r.CPV = r.cpvs(n.CPVMain, seen)
	r.CPV = append(r.CPV, r.cpvs(n.CPVAdditional, seen)...)
	for _, lot := range n.Lots {
		r.CPV = append(r.CPV, r.cpvs(lot.CPVMain, seen)...)
	}
	r.Changes = dedupeChanges(r.Changes)

	for _, raw := range n.NUTS {
		code, err := NUTS(raw)
		switch {
		case errors.Is(err, ErrNUTSExtraRegio):
			r.Problems = append(r.Problems, "nuts: a code is extra-regio, not a place")
			continue
		case err != nil:
			r.Problems = append(r.Problems, "nuts: a code is not a NUTS code")
			continue
		}
		if code != raw {
			r.Changes = append(r.Changes, Change{Field: "nuts", From: raw, To: code, Rule: RuleUpperCase})
		}
		r.NUTS = append(r.NUTS, code)
	}
	r.NUTS = dedupe(r.NUTS)

	r.BuyerOrgNumbers = r.orgNumbers("buyer org number", n.BuyerOrgNumbers)
	var winners []string
	for _, res := range n.Results {
		for _, w := range res.Winners {
			winners = append(winners, w.OrgNumber)
		}
	}
	r.WinnerOrgNumbers = r.orgNumbers("winner org number", winners)

	r.Problems = dedupe(r.Problems)
	r.Deadlines = deadlines(n)
	r.EarliestDeadline = earliest(r.Deadlines)
	return r
}

// cpvs normalises codes and records each change once. Codes already in seen are skipped; a nil
// seen keeps every code.
func (r *Result) cpvs(raw []string, seen map[string]bool) []string {
	var out []string
	for _, v := range raw {
		code, err := CPV(v)
		if err != nil {
			r.Problems = append(r.Problems, "cpv: a code is not a CPV code")
			continue
		}
		if code != v {
			r.Changes = append(r.Changes, Change{Field: "cpv", From: v, To: code, Rule: RuleCPVCheckDigit})
		}
		if seen != nil {
			if seen[code] {
				continue
			}
			seen[code] = true
		}
		out = append(out, code)
	}
	return out
}

func (r *Result) orgNumbers(field string, raw []string) []string {
	var out []string
	for _, v := range raw {
		if v == "" {
			continue
		}
		number, err := OrgNumber(v)
		switch {
		case errors.Is(err, ErrOrgNumberChecksum):
			r.Problems = append(r.Problems, field+": the check digit is wrong")
			continue
		case err != nil:
			r.Problems = append(r.Problems, field+": not a Norwegian organisation number")
			continue
		}
		if number != v {
			r.Changes = append(r.Changes, Change{Field: field, From: v, To: number, Rule: RuleOrgNumberForm})
		}
		out = append(out, number)
	}
	return dedupe(out)
}

// deadlines reads the deadline that binds for the notice and for each lot.
func deadlines(n eforms.Notice) []Deadline {
	var out []Deadline
	add := func(lot string, tender, participation *time.Time) {
		switch {
		case participation != nil:
			out = append(out, Deadline{Lot: lot, Kind: DeadlineParticipation, At: *participation})
		case tender != nil:
			out = append(out, Deadline{Lot: lot, Kind: DeadlineTender, At: *tender})
		}
	}
	add("", n.Deadline, n.ParticipationDeadline)
	for _, lot := range n.Lots {
		add(lot.ID, lot.Deadline, lot.ParticipationDeadline)
	}
	return out
}

// earliest is the earliest of the deadlines. Lot deadlines count because the captured notices
// state the deadline per lot, not for the notice.
func earliest(all []Deadline) *time.Time {
	var first *time.Time
	for _, d := range all {
		if first == nil || d.At.Before(*first) {
			at := d.At
			first = &at
		}
	}
	return first
}

func dedupe(in []string) []string {
	if in == nil {
		return nil
	}
	seen := map[string]bool{}
	out := make([]string, 0, len(in))
	for _, v := range in {
		if !seen[v] {
			seen[v] = true
			out = append(out, v)
		}
	}
	return out
}

func dedupeChanges(in []Change) []Change {
	seen := map[Change]bool{}
	var out []Change
	for _, c := range in {
		if !seen[c] {
			seen[c] = true
			out = append(out, c)
		}
	}
	return out
}
