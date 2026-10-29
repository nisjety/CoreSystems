package lifecycle

import (
	"fmt"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
)

const (
	// MaxChanges bounds how many changes Diff returns. The total is returned too.
	MaxChanges = 100
	// MaxTextRunes bounds each text in a change; a longer one is cut and ends in an ellipsis.
	MaxTextRunes = 300
)

// The operations of a change.
const (
	OpChanged = "changed"
	OpAdded   = "added"
	OpRemoved = "removed"
)

// Change is one thing that differs between two versions of a notice. Field names it: deadline,
// participation_deadline, value, cpv, nuts, lots, documents, requirements, selection_criteria,
// award_criteria, procedure_type, title or description. Lot is the lot it is about, or empty. Before
// and After are short texts, clipped; a description change gives its length and none of its text.
// DaysMoved is set for a deadline that moved: Oslo calendar days, later being positive. Material is
// false for wording and true for anything a bidder would act on.
type Change struct {
	Field     string
	Lot       string
	Op        string
	Before    string
	After     string
	DaysMoved *int
	Material  bool
}

// Diff says what differs between an earlier version of a notice and a later one, field by field. It
// returns at most MaxChanges, and the number there were. It reads no contact person: a Notice holds
// none.
func Diff(prev, next eforms.Notice) ([]Change, int) {
	var d differ
	d.deadline("deadline", "", prev.Deadline, next.Deadline)
	d.deadline("participation_deadline", "", prev.ParticipationDeadline, next.ParticipationDeadline)
	d.amount("", prev.EstimatedValue, next.EstimatedValue)
	d.lots(prev, next)
	d.set("cpv", "", cpvCodes(prev), cpvCodes(next))
	d.set("nuts", "", nutsCodes(prev), nutsCodes(next))
	d.items("documents", documentItems(prev), documentItems(next))
	d.items("requirements", requirementItems(prev), requirementItems(next))
	d.items("selection_criteria", selectionItems(prev), selectionItems(next))
	if prev.ProcedureType != next.ProcedureType {
		d.add(Change{Field: "procedure_type", Op: OpChanged, Before: clip(prev.ProcedureType), After: clip(next.ProcedureType), Material: true})
	}
	if prev.Title != next.Title {
		d.add(Change{Field: "title", Op: OpChanged, Before: clip(prev.Title), After: clip(next.Title)})
	}
	if prev.Description != next.Description {
		d.add(Change{Field: "description", Op: OpChanged,
			Before: fmt.Sprintf("%d characters", len([]rune(prev.Description))), After: fmt.Sprintf("%d characters", len([]rune(next.Description)))})
	}
	return d.out, d.total
}

// differ collects changes. It keeps the first MaxChanges and counts the rest, so a notice that
// changed in tens of thousands of places costs time in proportion to its size and memory in
// proportion to the bound.
type differ struct {
	out   []Change
	total int
}

func (d *differ) add(c Change) {
	d.total++
	if len(d.out) < MaxChanges {
		d.out = append(d.out, c)
	}
}

// deadline compares two instants. An instant that appears or goes is added or removed; one that moves
// is changed, with the days it moved.
func (d *differ) deadline(field, lot string, prev, next *time.Time) {
	switch {
	case prev == nil && next == nil:
	case prev == nil:
		d.add(Change{Field: field, Lot: lot, Op: OpAdded, After: instant(*next), Material: true})
	case next == nil:
		d.add(Change{Field: field, Lot: lot, Op: OpRemoved, Before: instant(*prev), Material: true})
	case !prev.Equal(*next):
		days := daysBetween(*prev, *next)
		d.add(Change{Field: field, Lot: lot, Op: OpChanged, Before: instant(*prev), After: instant(*next), DaysMoved: &days, Material: true})
	}
}

func instant(t time.Time) string { return t.UTC().Format(time.RFC3339) }

// daysBetween counts Oslo calendar days from a to b, later being positive. It counts days, not
// 24-hour spans, so a change to or from summer time does not shift it.
func daysBetween(a, b time.Time) int {
	x, errX := time.Parse(time.DateOnly, normalize.OsloDay(a))
	y, errY := time.Parse(time.DateOnly, normalize.OsloDay(b))
	if errX != nil || errY != nil {
		return 0
	}
	return int(y.Sub(x).Hours() / 24)
}

func (d *differ) amount(lot string, prev, next *eforms.Amount) {
	switch {
	case prev == nil && next == nil:
	case prev == nil:
		d.add(Change{Field: "value", Lot: lot, Op: OpAdded, After: money(*next), Material: true})
	case next == nil:
		d.add(Change{Field: "value", Lot: lot, Op: OpRemoved, Before: money(*prev), Material: true})
	case *prev != *next:
		d.add(Change{Field: "value", Lot: lot, Op: OpChanged, Before: money(*prev), After: money(*next), Material: true})
	}
}

func money(a eforms.Amount) string {
	return fmt.Sprintf("%d.%02d %s", a.Minor/100, a.Minor%100, a.Currency)
}

// lots reports lots added and removed, and for a lot in both, its deadlines, value and award criteria.
func (d *differ) lots(prev, next eforms.Notice) {
	before := map[string]eforms.Lot{}
	for _, l := range prev.Lots {
		before[l.ID] = l
	}
	after := map[string]eforms.Lot{}
	for _, l := range next.Lots {
		after[l.ID] = l
	}
	for _, l := range next.Lots {
		p, ok := before[l.ID]
		if !ok {
			d.add(Change{Field: "lots", Lot: l.ID, Op: OpAdded, After: clip(l.Title), Material: true})
			continue
		}
		d.deadline("deadline", l.ID, p.Deadline, l.Deadline)
		d.deadline("participation_deadline", l.ID, p.ParticipationDeadline, l.ParticipationDeadline)
		d.amount(l.ID, p.Value, l.Value)
		d.criteria(l.ID, p.AwardCriteria, l.AwardCriteria)
	}
	for _, l := range prev.Lots {
		if _, ok := after[l.ID]; !ok {
			d.add(Change{Field: "lots", Lot: l.ID, Op: OpRemoved, Before: clip(l.Title), Material: true})
		}
	}
}

// criteria compares the award criteria of one lot by what they are (type, name, description), and
// reports a weight that changed, and a criterion that was added or removed.
func (d *differ) criteria(lot string, prev, next []eforms.AwardCriterion) {
	key := func(c eforms.AwardCriterion) string { return c.Type + "\x00" + c.Name + "\x00" + c.Description }
	label := func(c eforms.AwardCriterion) string {
		name := c.Name
		if name == "" {
			name = c.Type
		}
		return name
	}
	weight := func(c eforms.AwardCriterion) string {
		if c.Weight == nil {
			return label(c) + ": no weight"
		}
		return fmt.Sprintf("%s: %d%%", label(c), *c.Weight)
	}
	before := map[string]eforms.AwardCriterion{}
	for _, c := range prev {
		before[key(c)] = c
	}
	after := map[string]bool{}
	for _, c := range next {
		after[key(c)] = true
		p, ok := before[key(c)]
		switch {
		case !ok:
			d.add(Change{Field: "award_criteria", Lot: lot, Op: OpAdded, After: clip(weight(c)), Material: true})
		case !sameWeight(p.Weight, c.Weight):
			d.add(Change{Field: "award_criteria", Lot: lot, Op: OpChanged, Before: clip(weight(p)), After: clip(weight(c)), Material: true})
		}
	}
	for _, c := range prev {
		if !after[key(c)] {
			d.add(Change{Field: "award_criteria", Lot: lot, Op: OpRemoved, Before: clip(weight(c)), Material: true})
		}
	}
}

func sameWeight(a, b *int) bool {
	if a == nil || b == nil {
		return a == nil && b == nil
	}
	return *a == *b
}

// set reports what was added to, and what was removed from, a set of short codes.
func (d *differ) set(field, lot string, prev, next []string) {
	before, after := membership(prev), membership(next)
	for _, c := range next {
		if !before[c] {
			d.add(Change{Field: field, Lot: lot, Op: OpAdded, After: clip(c), Material: true})
		}
	}
	for _, c := range prev {
		if !after[c] {
			d.add(Change{Field: field, Lot: lot, Op: OpRemoved, Before: clip(c), Material: true})
		}
	}
}

func membership(list []string) map[string]bool {
	m := make(map[string]bool, len(list))
	for _, v := range list {
		m[v] = true
	}
	return m
}

// item is an element of a list that is compared by its content: the lot it belongs to, the text that
// identifies it, and what is shown of it.
type item struct{ lot, key, text string }

// items reports what was added to, and what was removed from, a list of items, in a stable order.
func (d *differ) items(field string, prev, next []item) {
	id := func(it item) string { return it.lot + "\x00" + it.key }
	before, after := make(map[string]bool, len(prev)), make(map[string]bool, len(next))
	for _, it := range prev {
		before[id(it)] = true
	}
	for _, it := range next {
		after[id(it)] = true
	}
	for _, it := range next {
		if !before[id(it)] {
			d.add(Change{Field: field, Lot: it.lot, Op: OpAdded, After: clip(it.text), Material: true})
		}
	}
	for _, it := range prev {
		if !after[id(it)] {
			d.add(Change{Field: field, Lot: it.lot, Op: OpRemoved, Before: clip(it.text), Material: true})
		}
	}
}

func documentItems(n eforms.Notice) []item {
	var out []item
	for _, doc := range n.Documents {
		out = append(out, item{doc.Lot, doc.URI, doc.URI})
	}
	return out
}

func requirementItems(n eforms.Notice) []item {
	var out []item
	for _, r := range n.Requirements {
		text := strings.TrimSpace(r.Type + " " + r.Text)
		out = append(out, item{r.Lot, r.Type + "\x00" + r.Text, text})
	}
	return out
}

func selectionItems(n eforms.Notice) []item {
	var out []item
	for _, c := range n.SelectionCriteria {
		text := c.Name
		if c.Description != "" {
			text = strings.TrimSpace(c.Name + ": " + c.Description)
		}
		out = append(out, item{c.Lot, c.Type + "\x00" + c.Name + "\x00" + c.Description, text})
	}
	return out
}

// cpvCodes is every CPV code of the notice and its lots, without check digits, each once, sorted.
func cpvCodes(n eforms.Notice) []string {
	raw := append(slices.Clone(n.CPVMain), n.CPVAdditional...)
	for _, l := range n.Lots {
		raw = append(raw, l.CPVMain...)
	}
	return codes(raw, normalize.CPV)
}

func nutsCodes(n eforms.Notice) []string { return codes(n.NUTS, normalize.NUTS) }

func codes(raw []string, norm func(string) (string, error)) []string {
	set := map[string]bool{}
	for _, r := range raw {
		c, err := norm(r)
		if err != nil {
			c = strings.TrimSpace(r)
		}
		if c != "" {
			set[c] = true
		}
	}
	out := make([]string, 0, len(set))
	for c := range set {
		out = append(out, c)
	}
	sort.Strings(out)
	return out
}

// clip cuts a text to MaxTextRunes characters and marks the cut.
func clip(s string) string {
	r := []rune(s)
	if len(r) <= MaxTextRunes {
		return s
	}
	return string(r[:MaxTextRunes]) + "…"
}
