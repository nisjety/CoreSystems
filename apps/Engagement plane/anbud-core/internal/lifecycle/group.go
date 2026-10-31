package lifecycle

import (
	"sort"
	"strconv"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

// contractNotice is the root element of a notice that can be amended: a call for competition.
const contractNotice = "ContractNotice"

// Version is one stored contract notice: the Doffin id it is stored under, what it parsed to and when
// the store took it.
type Version struct {
	ID        string
	Notice    eforms.Notice
	FetchedAt time.Time
}

// Procedure is the notices of one procedure, oldest first. A notice that is not grouped, a result or a
// planning notice, is a procedure of its own with one version.
type Procedure struct {
	ID       string
	Versions []Version
}

// Original is the earliest notice held. It need not be the first the buyer published: an earlier one
// may have been issued before the store began.
func (p Procedure) Original() Version { return p.Versions[0] }

// Current is the latest notice held: the tender as it stands now.
func (p Procedure) Current() Version { return p.Versions[len(p.Versions)-1] }

// Amended reports whether the procedure holds more than one notice.
func (p Procedure) Amended() bool { return len(p.Versions) > 1 }

// Group puts contract notices that share a procedure identifier into one procedure, oldest first, and
// leaves every other notice alone. The same notice version stored twice counts once. The order of the
// result is the order in which each procedure first appears in versions.
func Group(versions []Version) []Procedure {
	index := map[string]int{}
	var out []Procedure
	for i, v := range versions {
		key := "row:" + strconv.Itoa(i)
		id := v.ID
		if v.Notice.Root == contractNotice && v.Notice.ProcedureID != "" {
			key, id = "procedure:"+v.Notice.ProcedureID, v.Notice.ProcedureID
		}
		at, ok := index[key]
		if !ok {
			at = len(out)
			index[key] = at
			out = append(out, Procedure{ID: id})
		}
		out[at].Versions = append(out[at].Versions, v)
	}
	for i := range out {
		out[i].Versions = settle(out[i].Versions)
	}
	return out
}

// settle orders versions oldest first and drops a version that repeats an earlier one's identifier and
// version number. The order is by issue time, then version number, then when it was stored, then the
// Doffin id: a total order, so it never depends on the order the store returned the rows in.
func settle(vs []Version) []Version {
	sort.SliceStable(vs, func(i, j int) bool { return older(vs[i], vs[j]) })
	seen := map[string]bool{}
	out := vs[:0]
	for _, v := range vs {
		if v.Notice.NoticeID != "" && v.Notice.VersionID != "" {
			key := v.Notice.NoticeID + "\x00" + v.Notice.VersionID
			if seen[key] {
				continue
			}
			seen[key] = true
		}
		out = append(out, v)
	}
	return out
}

func older(a, b Version) bool {
	if x, y := issued(a), issued(b); !x.Equal(y) {
		return x.Before(y)
	}
	if x, y := number(a), number(b); x != y {
		return x < y
	}
	if !a.FetchedAt.Equal(b.FetchedAt) {
		return a.FetchedAt.Before(b.FetchedAt)
	}
	return a.ID < b.ID
}

// issued is when a notice was issued; a notice that states no time is the oldest.
func issued(v Version) time.Time {
	if v.Notice.IssuedAt == nil {
		return time.Time{}
	}
	return *v.Notice.IssuedAt
}

// number is a notice's version number, or 0 when it is not one.
func number(v Version) int {
	n, err := strconv.Atoi(v.Notice.VersionID)
	if err != nil || n < 0 {
		return 0
	}
	return n
}

// Confirms reports whether next says, in its own change section, that it changes prev: it names prev's
// notice identifier and version. A TED publication number cannot confirm anything, because a stored
// body does not carry its own.
func Confirms(prev, next Version) bool {
	c := next.Notice.Change
	if c == nil {
		return false
	}
	id, version, ok := c.ReplacedNotice()
	return ok && id != "" && id == prev.Notice.NoticeID && version == prev.Notice.VersionID
}
