package incumbency

import (
	"sort"
	"time"

	"github.com/triodelab/anbud/internal/match"
)

// MaxReTenders is how many contracts the radar lists.
const MaxReTenders = 50

// Window is the span of days in which a contract's end puts it on the radar, both ends included.
type Window struct {
	From, To time.Time
}

// Scope narrows the radar to a profile's codes. An empty list does not narrow; a contract that states
// no code of a kind is not dropped for it, the way the gate treats an unknown.
type Scope struct {
	CPV  []string
	NUTS []string
	// Level is the slider's step: how far a code may be from the profile's and still be listed. The zero
	// value is strict, the step the radar took before the slider existed.
	Level match.Strictness
}

// ReTenders lists the contracts whose base period ends in the window: the ones a buyer is likely to
// tender again. Those the strictest step reaches come first, then those only a wider step reaches, each
// group nearest end first. A contract with an open or unknown end is not on it. Winners follow the
// panel's rule. It is Ending and then Show.
func ReTenders(awards []Award, s Scope, w Window, classes map[string]Class) []Entry {
	return Show(Ending(awards, s, w), classes)
}

// endingAll selects the awards whose contract ends in the window and lies in the scope, each awarded lot
// once, in the order ReTenders lists them. Matching and Widening count with it; Ending caps it for the list.
func endingAll(awards []Award, s Scope, w Window) []Award {
	type ending struct {
		a    Award
		tier int // 0 when the strictest step reaches the award's codes, 1 when only a wider step does
		end  time.Time
	}
	var found []ending
	seen := map[string]bool{}
	for _, a := range awards {
		if seen[a.key()] {
			continue
		}
		end := a.End()
		if end.At == nil || end.At.Before(w.From) || end.At.After(w.To) {
			continue
		}
		if !inScope(a.CPV, s.CPV, s.Level, true) || !inScope(a.NUTS, s.NUTS, s.Level, false) {
			continue
		}
		seen[a.key()] = true
		found = append(found, ending{a: a, tier: tierOf(a, s), end: *end.At})
	}
	sort.SliceStable(found, func(i, j int) bool {
		if found[i].tier != found[j].tier {
			return found[i].tier < found[j].tier
		}
		if !found[i].end.Equal(found[j].end) {
			return found[i].end.Before(found[j].end)
		}
		return found[i].a.NoticeID < found[j].a.NoticeID
	})
	out := make([]Award, 0, len(found))
	for _, f := range found {
		out = append(out, f.a)
	}
	return out
}

// tierOf is 0 when the strictest step reaches the award's codes, inside the scope's areas or covering
// them, and 1 when only a wider step does. The list is cut at the cap from the end, so a wider step
// never pushes an exact match off it.
func tierOf(a Award, s Scope) int {
	if inScope(a.CPV, s.CPV, match.Strict, true) && inScope(a.NUTS, s.NUTS, match.Strict, false) {
		return 0
	}
	return 1
}

// ExactMatch reports whether the strictest step already reaches the award's codes, inside the scope's areas
// or covering them. The radar marks such a contract exact, and one that only a wider step reaches near.
func ExactMatch(a Award, s Scope) bool { return tierOf(a, s) == 0 }

// Ending is the radar's list: endingAll, at most MaxReTenders. The caller can look up the winners
// (NorwegianWinners) before showing them.
func Ending(awards []Award, s Scope, w Window) []Award {
	all := endingAll(awards, s, w)
	return all[:min(len(all), MaxReTenders)]
}

// Matching is how many contracts the scope matches in the window before the cap: the total that a cut
// list is shown against.
func Matching(awards []Award, s Scope, w Window) int {
	return len(endingAll(awards, s, w))
}

// Widen is one wider step of the slider: how many more contracts it matches than the scope's own step,
// before the cap.
type Widen struct {
	Level match.Strictness
	Adds  int
}

// Widening counts, for each step wider than the scope's, the contracts it adds over the scope's own step
// in the window, before the cap.
func Widening(awards []Award, s Scope, w Window) []Widen {
	base := len(endingAll(awards, s, w))
	var out []Widen
	for level := s.Level + 1; level <= match.Wide; level++ {
		wider := s
		wider.Level = level
		out = append(out, Widen{Level: level, Adds: len(endingAll(awards, wider, w)) - base})
	}
	return out
}

// Show turns awards into entries, with their winners as the panel's rule allows.
func Show(awards []Award, classes map[string]Class) []Entry {
	out := make([]Entry, 0, len(awards))
	for _, a := range awards {
		e, _ := entry(a, "", classes)
		out = append(out, e)
	}
	return out
}

// NorwegianWinners lists the winners of the awards that the register can say something about:
// Norwegian organisation numbers, each once, at most limit of them, in the awards' order.
func NorwegianWinners(awards []Award, limit int) []string { return norwegianWinners(awards, limit) }

// inScope reports whether the award's codes reach the scope's at the step, by the rule the gate applies
// to a notice's codes. No scope, or no code on the award, does not narrow.
func inScope(codes, areas []string, level match.Strictness, cpv bool) bool {
	if len(areas) == 0 || len(codes) == 0 {
		return true
	}
	return match.CodeReaches(codes, areas, level, cpv)
}
