// Package switches is the one reader for audit-core's default-off switches.
//
// A switch turns on only for the exact literal "true". Every other value reads as
// off. Before this rule the readers took "1" as on, and the verifier read anything
// but "0" as on, so a live value that a retired reader took as on now reads as off
// without a trace. Check names that case at boot, and refuses to boot where the
// silent change could switch a feature on or off unseen.
package switches

import (
	"slices"
	"strings"

	"github.com/rs/zerolog/log"
)

// Literal is the one value an audit-core switch reads as on.
const Literal = "true"

// On reports whether raw turns a switch on. Only the exact literal does.
func On(raw string) bool {
	return raw == Literal
}

// formerOn are the spellings a retired lenient reader took as on, compared after
// trimming and case folding. The set matches mp_contracts::loop_fold::former_spelling.
var formerOn = []string{"1", "on", "yes", "enabled", "true"}

// offWords are the values that read as off under the retired readers and under the
// exact one alike, so a switch set to one of them has nothing to report.
var offWords = []string{"", "0", "false", "off", "no", "disabled"}

// FormerSpelling reports whether raw is a spelling a retired lenient reader took as
// on and the exact reader does not: 1, on, yes, enabled, or true in another case or
// with surrounding space.
func FormerSpelling(raw string) bool {
	return raw != Literal && slices.Contains(formerOn, fold(raw))
}

// Misread reports whether raw reads as off although it says something other than
// off: a former spelling of on, or a typo such as ture. Blank and the off words are
// not misread.
func Misread(raw string) bool {
	return raw != Literal && !slices.Contains(offWords, fold(raw))
}

func fold(raw string) string {
	return strings.ToLower(strings.TrimSpace(raw))
}

// Refusal says what boot does with a switch that is set to a misread value.
type Refusal int

const (
	// WarnOnly logs the misread value and boots with the switch off. It is for a
	// switch whose reader was already exact, so its behaviour does not change.
	WarnOnly Refusal = iota
	// RefuseInProduction also refuses to boot when ENVIRONMENT=production, and only
	// warns elsewhere. It is for a switch whose reader this change moved off a lenient
	// one, so a former spelling would silently change what the service does.
	RefuseInProduction
	// RefuseAlways refuses to boot in every environment. It is for a guard, such as a
	// dry run, whose silent flip could let a deletion through.
	RefuseAlways
)

// Switch is one audit-core switch and what boot does with a misread value.
type Switch struct {
	Name   string
	Refuse Refusal
}

// SwitchError is the typed config error that boot returns for a misread switch it
// refuses. The message names the switch and never its value.
type SwitchError struct {
	Name string
}

func (e *SwitchError) Error() string {
	return e.Name + " must be true or false"
}

// Check runs once at boot, before any switch is read. For each switch in list that is
// set to a misread value, it logs a warning that names the switch and never its value.
// It returns the first *SwitchError for a switch that refuses under the given
// environment. Nothing is turned on: a misread value still reads as off.
func Check(getenv func(string) string, production bool, list []Switch) error {
	var refusal error
	for _, s := range list {
		raw := getenv(s.Name)
		if !Misread(raw) {
			continue
		}
		log.Warn().
			Str("switch", s.Name).
			Str("exact_value", Literal).
			Bool("former_spelling", FormerSpelling(raw)).
			Msg("switch is set to a value other than its exact literal, so it reads as off; set it to exactly true to turn it on")
		refuses := s.Refuse == RefuseAlways || (s.Refuse == RefuseInProduction && production)
		if refuses && refusal == nil {
			refusal = &SwitchError{Name: s.Name}
		}
	}
	return refusal
}
