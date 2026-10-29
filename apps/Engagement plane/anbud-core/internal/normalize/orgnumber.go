package normalize

import (
	"errors"
	"strings"
)

var (
	// ErrOrgNumber means a value is not a Norwegian organisation number: nine digits, optionally
	// written with spaces or dots, a leading "NO" or a trailing "MVA".
	ErrOrgNumber = errors.New("normalize: not a Norwegian organisation number")
	// ErrOrgNumberChecksum means nine digits whose check digit is wrong: a typo, or a number that
	// was never issued.
	ErrOrgNumberChecksum = errors.New("normalize: organisation number check digit is wrong")
)

// orgWeights are Brønnøysundregistrene's modulus-11 weights for the first eight digits.
var orgWeights = [8]int{3, 2, 7, 6, 5, 4, 3, 2}

// OrgNumber returns the nine digits of a Norwegian organisation number and verifies its check
// digit. The VAT spelling ("NO 974 760 673 MVA") is the same number. A foreign identifier is
// refused: it is not an organisation number this package can check.
func OrgNumber(raw string) (string, error) {
	s := strings.ToUpper(strings.TrimSpace(raw))
	s = strings.TrimPrefix(s, "NO")
	s = strings.TrimSuffix(s, "MVA")
	s = strings.NewReplacer(" ", "", ".", "", "\xc2\xa0", "").Replace(s)
	if len(s) != 9 {
		return "", ErrOrgNumber
	}
	digits := []byte(s)
	for _, d := range digits {
		if d < '0' || d > '9' {
			return "", ErrOrgNumber
		}
	}
	check := checkDigit(digits[:8])
	if check < 0 || int(digits[8]-'0') != check {
		return "", ErrOrgNumberChecksum
	}
	return s, nil
}

// checkDigit is the modulus-11 check digit of eight digits, or -1 when the remainder leaves a
// check digit of 10, which no valid number has.
func checkDigit(stem []byte) int {
	sum := 0
	for i, d := range stem {
		sum += int(d-'0') * orgWeights[i]
	}
	check := 11 - sum%11
	switch check {
	case 11:
		return 0
	case 10:
		return -1
	}
	return check
}
