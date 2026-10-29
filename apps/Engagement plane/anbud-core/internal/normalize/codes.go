package normalize

import (
	"errors"
	"regexp"
	"strings"
)

var (
	// ErrCPV means a value is not a CPV code: eight digits, optionally with a dash and one check digit.
	ErrCPV = errors.New("normalize: not a CPV code")
	// ErrNUTS means a value is not a NUTS code: a country code and up to three more levels.
	ErrNUTS = errors.New("normalize: not a NUTS code")
	// ErrNUTSExtraRegio means a NUTS code ends in Z, which Eurostat uses for "extra-regio": it names
	// no place, so it cannot match a region.
	ErrNUTSExtraRegio = errors.New("normalize: an extra-regio NUTS code names no place")
)

var (
	cpvForm  = regexp.MustCompile(`^(\d{8})(?:-\d)?$`)
	nutsForm = regexp.MustCompile(`^[A-Z]{2}[A-Z0-9]{0,3}$`)
)

// CPV returns the eight digits of a CPV code. The check digit is dropped: eForms states codes
// without it, so two spellings of one code compare equal only without it. The check digit is not
// verified, because the CPV regulation does not publish its algorithm.
func CPV(raw string) (string, error) {
	m := cpvForm.FindStringSubmatch(strings.TrimSpace(raw))
	if m == nil {
		return "", ErrCPV
	}
	return m[1], nil
}

// CPVAncestors returns the codes above a CPV code, nearest first, down to its division. CPV is a
// hierarchy written in its digits: a code's level is its last non-zero digit, and its parent is the
// code with that digit set to zero, from the third digit up (division, group, class, category).
// This is how a search widens: 72212100 → 72212000 → 72210000 → 72200000 → 72000000.
func CPVAncestors(code string) []string {
	if !cpvForm.MatchString(code) || len(code) != 8 {
		return nil
	}
	digits := []byte(code)
	var out []string
	for i := 7; i >= 2; i-- {
		if digits[i] == '0' {
			continue
		}
		digits[i] = '0'
		out = append(out, string(digits))
	}
	return out
}

// NUTS returns a NUTS code in upper case. An extra-regio code (one ending in Z) is refused with
// ErrNUTSExtraRegio, so it is never matched as if it were a place.
func NUTS(raw string) (string, error) {
	code := strings.ToUpper(strings.TrimSpace(raw))
	if !nutsForm.MatchString(code) {
		return "", ErrNUTS
	}
	if len(code) > 2 && strings.HasSuffix(code, "Z") {
		return "", ErrNUTSExtraRegio
	}
	return code, nil
}

// NUTSAncestors returns the regions above a NUTS code, nearest first, down to the country:
// NO081 → NO08 → NO0 → NO. Each level is one more character.
func NUTSAncestors(code string) []string {
	if !nutsForm.MatchString(code) {
		return nil
	}
	var out []string
	for n := len(code) - 1; n >= 2; n-- {
		out = append(out, code[:n])
	}
	return out
}
