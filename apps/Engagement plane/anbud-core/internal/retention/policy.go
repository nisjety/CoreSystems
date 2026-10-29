// Package retention decides when a stored notice body has been kept long enough to be erased
// (open owner decision O1, docs/PERSONAL-DATA.md). A body is kept for a number of months after the
// latest date it is still useful for: the end of a contract it awards, else a deadline it sets,
// else the day it was awarded or issued, else the day it was fetched. A contract the notice says has
// no end keeps the body. The package is pure: it reads a body and decides, and erases nothing.
//
// The period is not decided here. It is a parameter, because retention is an owner and compliance
// decision (open question 3 of the runbook), and the command that applies it asks for it every run.
package retention

import (
	"errors"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/incumbency"
)

// MaxMonths bounds the period: ten years is longer than any contract the corpus has shown, and a
// larger number is a typing mistake.
const MaxMonths = 120

// ErrMonths refuses a period outside 1 to MaxMonths.
var ErrMonths = errors.New("retention: the period must be between 1 and 120 months")

// Basis names the date a decision counts from.
type Basis string

const (
	BasisContractEnd Basis = "contract_end" // the latest end of a contract the notice awards
	BasisDeadline    Basis = "deadline"     // the latest deadline for tenders or for requests to participate
	BasisAwarded     Basis = "awarded"      // the award day
	BasisIssued      Basis = "issued"       // the issue time of the notice
	BasisPlanned     Basis = "planned"      // the day a prior information notice says its notice will be published
	BasisFetched     Basis = "fetched"      // the day it was fetched: the notice could not be read, or gives no date
	BasisOpen        Basis = "open"         // a contract the notice awards has no end: kept
)

// Decision is what the policy says about one body. From and ExpiresAt are zero for BasisOpen, and
// Expired is then false.
type Decision struct {
	Basis     Basis
	From      time.Time
	ExpiresAt time.Time
	Expired   bool
}

// Decide parses body and decides. A body the parser refuses counts from fetchedAt: the policy
// cannot tell what it is for, and keeping it for ever because it is unreadable is not a rule.
func Decide(body []byte, fetchedAt time.Time, months int, asOf time.Time) (Decision, error) {
	n, err := eforms.Parse(body)
	return decide(n, err == nil, fetchedAt, months, asOf)
}

// decide is Decide for a notice already parsed; parsed is false when the parser refused it.
func decide(n eforms.Notice, parsed bool, fetchedAt time.Time, months int, asOf time.Time) (Decision, error) {
	if months < 1 || months > MaxMonths {
		return Decision{}, ErrMonths
	}
	basis, from := BasisFetched, fetchedAt.UTC()
	if parsed {
		var open bool
		basis, from, open = latest(n, basis, from)
		if open {
			return Decision{Basis: BasisOpen}, nil
		}
	}
	expires := from.AddDate(0, months, 0)
	return Decision{Basis: basis, From: from, ExpiresAt: expires, Expired: !asOf.UTC().Before(expires)}, nil
}

// latest is the latest useful date a notice states, with its basis, or the fallback when it states
// none. open is true when a contract it awards has no end. The bases are tried in order and a later
// date always wins, so the body is kept for the longest of its uses.
func latest(n eforms.Notice, fallback Basis, at time.Time) (Basis, time.Time, bool) {
	var (
		basis Basis
		best  time.Time
	)
	consider := func(b Basis, t *time.Time) {
		if t != nil && t.After(best) {
			basis, best = b, t.UTC()
		}
	}
	for _, a := range incumbency.FromNotice("", n) {
		end := a.End()
		if end.Basis == incumbency.EndOpen {
			return "", time.Time{}, true
		}
		consider(BasisContractEnd, end.At)
	}
	consider(BasisDeadline, n.Deadline)
	consider(BasisDeadline, n.ParticipationDeadline)
	for _, l := range n.Lots {
		consider(BasisDeadline, l.Deadline)
		consider(BasisDeadline, l.ParticipationDeadline)
	}
	consider(BasisPlanned, n.PlannedNoticeDate)
	consider(BasisAwarded, n.AwardDate)
	consider(BasisIssued, n.IssuedAt)
	if basis == "" {
		return fallback, at, false
	}
	return basis, best, false
}
