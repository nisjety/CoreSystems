package demo

import (
	"fmt"
	"strconv"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/lifecycle"
)

// An amended tender is one procedure with several contract notices. The list shows its current notice,
// the latest issued, and counts the earlier ones as superseded; the audit sheet shows all of them.
// Nothing is stored: the notices of a procedure are grouped on every request, from what the store
// holds, so an erasure takes effect on the next request as it does everywhere else.

// versionKey names a candidate inside a lifecycle.Version by its position. It is fixed width, so the
// lifecycle's ordering by id puts equal notices in the order they were read, and it stays unique when
// the store holds two rows under one Doffin id.
func versionKey(i int) string { return fmt.Sprintf("%08d", i) }

// indexOf is the position of a version's candidate. The keys are made by versionKey, so the parse
// cannot fail for a version built here.
func indexOf(v lifecycle.Version) int {
	i, _ := strconv.Atoi(v.ID)
	return i
}

// settled is the outcome of grouping the contract notices a scan read by procedure.
type settled struct {
	cs         []candidate
	groups     []lifecycle.Procedure
	current    []candidate
	superseded int
	// amended holds, by the Doffin id of a current notice, the procedure of a tender that was amended.
	amended map[string]lifecycle.Procedure
}

// settle groups the contract notices by procedure and keeps the latest of each procedure as the
// current tender. A notice that is not the latest of its procedure is superseded.
func settle(cs []candidate) settled {
	versions := make([]lifecycle.Version, len(cs))
	for i, c := range cs {
		versions[i] = lifecycle.Version{ID: versionKey(i), Notice: c.Notice, FetchedAt: c.fetched}
	}
	s := settled{cs: cs, groups: lifecycle.Group(versions), amended: map[string]lifecycle.Procedure{}}
	for _, p := range s.groups {
		c := cs[indexOf(p.Current())]
		s.current = append(s.current, c)
		s.superseded += len(p.Versions) - 1
		if p.Amended() {
			s.amended[c.id] = p
		}
	}
	return s
}

// groupOf returns the procedure the candidate at position i belongs to.
func (s settled) groupOf(i int) (lifecycle.Procedure, bool) {
	key := versionKey(i)
	for _, p := range s.groups {
		for _, v := range p.Versions {
			if v.ID == key {
				return p, true
			}
		}
	}
	return lifecycle.Procedure{}, false
}

// amendmentsOf is the history of the audited notice's procedure, from the contract notices the scan
// read. The audited notice takes the place of any copy the scan read, so it is judged as it was asked
// for. It is nil for a notice that nobody amended.
func amendmentsOf(c candidate, scanned []candidate) *AmendmentsView {
	others := make([]candidate, 0, len(scanned)+1)
	for _, o := range scanned {
		if o.id != c.id {
			others = append(others, o)
		}
	}
	others = append(others, c)
	set := settle(others)
	at := len(others) - 1
	p, ok := set.groupOf(at)
	if !ok || len(p.Versions) < 2 {
		return nil
	}
	return historyView(p, set.cs, versionKey(at))
}

// historyView is a procedure's notices, oldest first, each with what it changed since the one before
// it, and where the audited notice sits among them.
func historyView(p lifecycle.Procedure, cs []candidate, audited string) *AmendmentsView {
	v := &AmendmentsView{ProcedureID: p.ID, Versions: []VersionView{}}
	for j, ver := range p.Versions {
		view := VersionView{
			NoticeID: cs[indexOf(ver)].id, Issued: issuedText(ver.Notice.IssuedAt),
			Current: j == len(p.Versions)-1, Changes: []NoticeChangeView{},
		}
		view.ReasonCode, view.Reason = reasonOf(ver.Notice)
		if j > 0 {
			prev := p.Versions[j-1]
			view.Confirmed = lifecycle.Confirms(prev, ver)
			changes, total := lifecycle.Diff(prev.Notice, ver.Notice)
			view.Changes, view.ChangesTotal = changeViews(changes), total
		}
		v.Versions = append(v.Versions, view)
		if ver.ID == audited {
			v.Position = j + 1
			if j+1 < len(p.Versions) {
				v.SupersededBy = cs[indexOf(p.Versions[j+1])].id
			}
		}
	}
	return v
}

// amendmentOf is what the latest notice of an amended tender changed. The changes compare it with the
// first notice held, so they are what has moved since the tender was first seen.
func amendmentOf(p lifecycle.Procedure) *AmendmentView {
	n := len(p.Versions)
	last := p.Current()
	code, reason := reasonOf(last.Notice)
	changes, total := lifecycle.Diff(p.Original().Notice, last.Notice)
	return &AmendmentView{
		Versions: n, ReasonCode: code, Reason: reason,
		Confirmed:  lifecycle.Confirms(p.Versions[n-2], last),
		LastIssued: issuedText(last.Notice.IssuedAt),
		Changes:    changeViews(changes), ChangesTotal: total,
	}
}

// reasonOf is the buyer's reason for a change notice, in its own words, clipped. A notice that is not a
// change has none.
func reasonOf(n eforms.Notice) (code, reason string) {
	if n.Change == nil {
		return "", ""
	}
	return n.Change.ReasonCode, clip(n.Change.Reason, maxTextRunes)
}

func changeViews(cs []lifecycle.Change) []NoticeChangeView {
	out := make([]NoticeChangeView, 0, len(cs))
	for _, c := range cs {
		out = append(out, NoticeChangeView{
			Field: c.Field, Lot: c.Lot, Op: c.Op, Before: c.Before, After: c.After,
			DaysMoved: c.DaysMoved, Material: c.Material,
		})
	}
	return out
}

// issuedText is an issue instant in UTC, or empty when the notice states none.
func issuedText(t *time.Time) string {
	if t == nil {
		return ""
	}
	return t.UTC().Format(time.RFC3339)
}

// AmendmentView is what the latest notice of an amended tender changed: how many notices its procedure
// holds, the buyer's reason in its own words, whether that notice names the one before it, and what has
// changed since the tender was first held.
type AmendmentView struct {
	Versions     int                `json:"versions"`
	ReasonCode   string             `json:"reason_code,omitempty"`
	Reason       string             `json:"reason,omitempty"`
	Confirmed    bool               `json:"confirmed"`
	LastIssued   string             `json:"last_issued,omitempty"`
	Changes      []NoticeChangeView `json:"changes"`
	ChangesTotal int                `json:"changes_total"`
}

// NoticeChangeView is one thing that differs between two notices of a procedure. Op is changed, added or
// removed; DaysMoved is set for a deadline that moved (Oslo calendar days, later being positive);
// Material is false for wording.
type NoticeChangeView struct {
	Field     string `json:"field"`
	Lot       string `json:"lot,omitempty"`
	Op        string `json:"op"`
	Before    string `json:"before,omitempty"`
	After     string `json:"after,omitempty"`
	DaysMoved *int   `json:"days_moved,omitempty"`
	Material  bool   `json:"material"`
}

// VersionView is one notice of a procedure, oldest first. Changes compare it with the notice before it.
type VersionView struct {
	NoticeID     string             `json:"notice_id"`
	Issued       string             `json:"issued,omitempty"`
	ReasonCode   string             `json:"reason_code,omitempty"`
	Reason       string             `json:"reason,omitempty"`
	Confirmed    bool               `json:"confirmed"`
	Current      bool               `json:"current"`
	Changes      []NoticeChangeView `json:"changes"`
	ChangesTotal int                `json:"changes_total"`
}

// AmendmentsView is the history of the audited notice's procedure. Position is the audited notice's
// place among the notices held, from 1; SupersededBy is the notice that replaces it, or empty when it
// is the current one.
type AmendmentsView struct {
	ProcedureID  string        `json:"procedure_id"`
	Position     int           `json:"position"`
	SupersededBy string        `json:"superseded_by,omitempty"`
	Versions     []VersionView `json:"versions"`
}
