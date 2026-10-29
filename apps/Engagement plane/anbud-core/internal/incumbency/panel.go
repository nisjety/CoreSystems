package incumbency

import (
	"slices"
	"sort"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
)

const (
	// MaxEntries is how many related awards a panel lists. The counts cover them all.
	MaxEntries = 20
	// maxTopWinners is how many organisations a panel ranks.
	maxTopWinners = 5
)

// Class is what the register says a winner is.
type Class int

const (
	// Unverified means the winner could not be checked: no lookup, a foreign identifier, or no
	// answer. It is treated as a sole trader might be: not named and not ranked.
	Unverified Class = iota
	// Organisation means the register holds it under a form that is not a sole trader's.
	Organisation
	// SoleTrader means the register holds it as an enkeltpersonforetak: its name is a person's.
	SoleTrader
)

func (c Class) String() string {
	switch c {
	case Organisation:
		return "organisation"
	case SoleTrader:
		return "sole_trader"
	}
	return "unverified"
}

// Relation is how close a past award is to the tender: the same CPV code, the same class (four
// digits) or the same group (three). Sharing only a division is too broad to say anything.
type Relation string

const (
	SameCPV   Relation = "same_cpv"
	SameClass Relation = "same_class"
	SameGroup Relation = "same_group"
)

var relationRank = map[Relation]int{SameCPV: 0, SameClass: 1, SameGroup: 2}

// Target is the tender a panel is for.
type Target struct {
	ProcedureID     string // its own procedure: the result of this very tender is not its history
	BuyerOrgNumbers []string
	CPV             []string
}

// Relate says whether a past award is history for the target: the same buyer, another procedure, and
// a CPV code that shares at least its group with one of the target's. It returns the closest relation.
func Relate(t Target, a Award) (Relation, bool) {
	if t.ProcedureID != "" && a.ProcedureID == t.ProcedureID {
		return "", false
	}
	if !overlap(t.BuyerOrgNumbers, a.BuyerOrgNumbers) {
		return "", false
	}
	best := ""
	for _, x := range t.CPV {
		for _, y := range a.CPV {
			var r Relation
			switch shared := commonDigits(x, y); {
			case shared == len(x) && x == y:
				r = SameCPV
			case shared >= 4:
				r = SameClass
			case shared == 3:
				r = SameGroup
			default:
				continue
			}
			if best == "" || relationRank[r] < relationRank[Relation(best)] {
				best = string(r)
			}
		}
	}
	return Relation(best), best != ""
}

func overlap(a, b []string) bool {
	for _, x := range a {
		if x != "" && slices.Contains(b, x) {
			return true
		}
	}
	return false
}

func commonDigits(a, b string) int {
	n := 0
	for n < len(a) && n < len(b) && a[n] == b[n] {
		n++
	}
	return n
}

// WinnerView is a winner as it may be shown: named only when it is an organisation.
type WinnerView struct {
	Name      string
	OrgNumber string
	Class     Class
}

func viewWinners(ws []eforms.Winner, classes map[string]Class) (views []WinnerView, withheld int) {
	for _, w := range ws {
		c := classes[w.OrgNumber]
		if w.OrgNumber == "" {
			c = Unverified
		}
		if c == Organisation {
			views = append(views, WinnerView{Name: w.Name, OrgNumber: w.OrgNumber, Class: c})
			continue
		}
		views = append(views, WinnerView{Class: c})
		withheld++
	}
	return views, withheld
}

// Entry is a past award as a panel or the radar shows it.
type Entry struct {
	NoticeID        string
	Title           string
	LotTitle        string
	BuyerNames      []string
	Relation        Relation // empty on the radar, which relates by the profile and not by a tender
	AwardDate       *time.Time
	Signed          *time.Time
	End             End
	Renewals        *int
	TendersReceived *int
	Value           *eforms.Amount
	Winners         []WinnerView
}

func entry(a Award, r Relation, classes map[string]Class) (Entry, int) {
	winners, withheld := viewWinners(a.Winners, classes)
	return Entry{
		NoticeID: a.NoticeID, Title: a.Title, LotTitle: a.LotTitle, BuyerNames: a.BuyerNames, Relation: r,
		AwardDate: a.AwardDate, Signed: a.Signed, End: a.End(), Renewals: a.Renewals,
		TendersReceived: a.TendersReceived, Value: a.Value, Winners: winners,
	}, withheld
}

// WinnerCount is an organisation and how many of the related awards it won.
type WinnerCount struct {
	Name      string
	OrgNumber string
	Wins      int
}

// Panel is a tender's history with its buyer: the related past awards, the likely incumbent, the
// organisations that won most, and how many tenders those competitions drew.
type Panel struct {
	Related int // every related award; Entries lists at most MaxEntries of them, newest first
	Entries []Entry
	// Incumbent is the latest award of the closest relation, as Entries shows it, or nil. It is
	// "likely": a later award outside the scanned notices, or one under another CPV, can supersede it.
	Incumbent  *Entry
	TopWinners []WinnerCount
	// WithheldWinners counts the winners not named in any related award: sole traders and winners
	// that could not be checked.
	WithheldWinners int
	// MedianTendersReceived is the median of the tender counts the related awards state, or nil when
	// none states one. It is a decimal: for an even number of counts it is the mean of the two in the
	// middle, so two and three give 2.5.
	MedianTendersReceived *float64
}

type related struct {
	a Award
	r Relation
}

// Build makes the panel of the target from the awards. classes says what the register holds for each
// winner's organisation number; a number not in it is unverified.
func Build(t Target, awards []Award, classes map[string]Class) Panel {
	var found []related
	seen := map[string]bool{}
	for _, a := range awards {
		if seen[a.key()] {
			continue
		}
		if r, ok := Relate(t, a); ok {
			seen[a.key()] = true
			found = append(found, related{a, r})
		}
	}
	p := Panel{Related: len(found)}
	if len(found) == 0 {
		return p
	}

	sort.SliceStable(found, func(i, j int) bool { return newer(found[i].a, found[j].a) })
	wins := map[string]*WinnerCount{}
	var tenders []int
	for i, f := range found {
		e, withheld := entry(f.a, f.r, classes)
		p.WithheldWinners += withheld
		if i < MaxEntries {
			p.Entries = append(p.Entries, e)
		}
		for _, w := range e.Winners {
			if w.Class != Organisation {
				continue
			}
			if wins[w.OrgNumber] == nil {
				wins[w.OrgNumber] = &WinnerCount{Name: w.Name, OrgNumber: w.OrgNumber}
			}
			wins[w.OrgNumber].Wins++
		}
		if f.a.TendersReceived != nil {
			tenders = append(tenders, *f.a.TendersReceived)
		}
	}

	incumbent := found[0]
	for _, f := range found[1:] {
		if relationRank[f.r] < relationRank[incumbent.r] {
			incumbent = f
		}
	}
	e, _ := entry(incumbent.a, incumbent.r, classes)
	p.Incumbent = &e

	for _, w := range wins {
		p.TopWinners = append(p.TopWinners, *w)
	}
	sort.Slice(p.TopWinners, func(i, j int) bool {
		if p.TopWinners[i].Wins != p.TopWinners[j].Wins {
			return p.TopWinners[i].Wins > p.TopWinners[j].Wins
		}
		return p.TopWinners[i].Name < p.TopWinners[j].Name
	})
	if len(p.TopWinners) > maxTopWinners {
		p.TopWinners = p.TopWinners[:maxTopWinners]
	}
	if len(tenders) > 0 {
		m := median(tenders)
		p.MedianTendersReceived = &m
	}
	return p
}

// newer orders awards newest first by the day they took effect; an award with no day comes last, and
// the notice id breaks a tie so the order is stable.
func newer(a, b Award) bool {
	x, y := a.when(), b.when()
	switch {
	case x != nil && y != nil && !x.Equal(*y):
		return x.After(*y)
	case (x == nil) != (y == nil):
		return x != nil
	}
	return a.NoticeID > b.NoticeID
}

// WinnersToCheck lists the winners of the target's related awards that the register can say
// something about: Norwegian organisation numbers, each once, at most limit of them, newest awards
// first. A foreign identifier is not listed.
func WinnersToCheck(t Target, awards []Award, limit int) []string {
	var found []Award
	for _, a := range awards {
		if _, ok := Relate(t, a); ok {
			found = append(found, a)
		}
	}
	sort.SliceStable(found, func(i, j int) bool { return newer(found[i], found[j]) })
	return norwegianWinners(found, limit)
}

func norwegianWinners(awards []Award, limit int) []string {
	var out []string
	seen := map[string]bool{}
	for _, a := range awards {
		for _, w := range a.Winners {
			number, err := normalize.OrgNumber(w.OrgNumber)
			if err != nil || number != w.OrgNumber || seen[number] {
				continue
			}
			if len(out) >= limit {
				return out
			}
			seen[number] = true
			out = append(out, number)
		}
	}
	return out
}

// median is the middle of the values: the middle one for an odd number, and for an even number the
// mean of the two in the middle. It does not reorder values. It needs at least one.
func median(values []int) float64 {
	sorted := slices.Clone(values)
	slices.Sort(sorted)
	n := len(sorted)
	if n%2 == 1 {
		return float64(sorted[n/2])
	}
	return float64(sorted[n/2-1]+sorted[n/2]) / 2
}
