package match

import (
	"testing"

	"github.com/triodelab/anbud/internal/normalize"
)

// reachCase is a notice code and a profile code, and whether they reach each other at each strictness.
type reachCase struct {
	notice, profile        string
	strict, balanced, wide bool
}

var reachCPVCases = []reachCase{
	{"72212000", "72212000", true, true, true},   // the same code
	{"72212100", "72212000", true, true, true},   // a narrower code, inside the profile's area
	{"72000000", "72212000", true, true, true},   // a division covering the profile's code
	{"72215000", "72212000", false, true, true},  // the same group: the first three digits
	{"72300000", "72212000", false, false, true}, // the same division only
	{"45000000", "72212000", false, false, false},
}

var reachRegionCases = []reachCase{
	{"NO081", "NO081", true, true, true},
	{"NO082", "NO081", false, true, true},  // the same NUTS 2 region
	{"NO0A1", "NO081", false, false, true}, // the same country part only
	{"SE111", "NO081", false, false, false},
	{"SE21", "SE11", false, false, false}, // another NUTS 1 region of the same country: not the same country part
}

func (c reachCase) at() map[Strictness]bool {
	return map[Strictness]bool{Strict: c.strict, Balanced: c.balanced, Wide: c.wide}
}

func TestCodeReachesStepsWithTheSliderForCPVCodes(t *testing.T) {
	t.Parallel()

	for _, c := range reachCPVCases {
		for level, want := range c.at() {
			if got := CodeReaches([]string{c.notice}, []string{c.profile}, level, true); got != want {
				t.Errorf("CPV %s against %s at %v: reaches %v, want %v", c.notice, c.profile, level, got, want)
			}
		}
	}
}

func TestCodeReachesStepsWithTheSliderForRegions(t *testing.T) {
	t.Parallel()

	for _, c := range reachRegionCases {
		for level, want := range c.at() {
			if got := CodeReaches([]string{c.notice}, []string{c.profile}, level, false); got != want {
				t.Errorf("region %s against %s at %v: reaches %v, want %v", c.notice, c.profile, level, got, want)
			}
		}
	}
}

// The gate and the radar apply one rule: a code that the CPV or region check does not fail is a code
// CodeReaches reaches, at every strictness. A change to either side is caught here.
func TestTheGateAndTheRadarApplyTheSameReach(t *testing.T) {
	t.Parallel()

	for level := Strict; level <= Wide; level++ {
		for _, c := range reachCPVCases {
			check := cpvCheck(normalize.Result{CPV: []string{c.notice}}, Profile{CPV: []string{c.profile}}, level)
			if (check.Outcome != Fail) != CodeReaches([]string{c.notice}, []string{c.profile}, level, true) {
				t.Errorf("CPV %s against %s at %v: the gate says %v, the radar's rule says otherwise", c.notice, c.profile, level, check.Outcome)
			}
		}
		for _, c := range reachRegionCases {
			check := regionCheck(normalize.Result{NUTS: []string{c.notice}}, Profile{NUTS: []string{c.profile}}, level)
			if (check.Outcome != Fail) != CodeReaches([]string{c.notice}, []string{c.profile}, level, false) {
				t.Errorf("region %s against %s at %v: the gate says %v, the radar's rule says otherwise", c.notice, c.profile, level, check.Outcome)
			}
		}
	}
}
