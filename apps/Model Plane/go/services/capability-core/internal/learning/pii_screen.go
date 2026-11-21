package learning

import (
	"context"
	"regexp"
	"strings"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"
)

// candidatesScreened counts the candidates the PII screen drops, labelled by the class of the first match. The class is
// a closed word: the match, where it sits and the candidate never reach the meter.
var candidatesScreened, _ = otel.Meter(telemetry.MeterName).Int64Counter(
	"learning_review_candidates_screened_total",
	metric.WithDescription("Learned skill candidates dropped by the PII and secret screen, labelled by class."),
)

// PIIScreen is the deterministic PII and secret screen for learned skill candidates. It is what the switch
// LEARNING_PII_SCREEN_ENABLED installs as Screen. It returns true to keep a candidate and false to drop it. A candidate
// is dropped and never redacted: an edited skill would be a different skill from the one the owner is asked to accept.
func PIIScreen(c SkillCandidate) bool {
	class := PIIClass(c)
	if class == "" {
		return true
	}
	candidatesScreened.Add(context.Background(), 1, metric.WithAttributes(attribute.String("class", class)))
	return false
}

// PIIClass names the first class of personal data or secret the candidate carries, or returns "" when it carries none.
// The classes are tried in a fixed order: secret, email, iban, national_id, card, phone. Only the class is returned,
// never the match, its position or the candidate.
func PIIClass(c SkillCandidate) string {
	text := screenedText(c)
	for _, detector := range piiDetectors {
		if detector.hit(text) {
			return detector.class
		}
	}
	return ""
}

// piiDetector is one class of personal data or secret and the test that finds it in the screened text.
type piiDetector struct {
	class string
	hit   func(text string) bool
}

// piiDetectors are tried in order by PIIClass. Secrets come first, so a credential that happens to contain a run of
// digits is named a secret rather than a card. The IBAN test comes before the national identity and card tests for the
// same reason.
var piiDetectors = []piiDetector{
	{"secret", hasSecret},
	{"email", hasEmail},
	{"iban", hasIBAN},
	{"national_id", hasNationalID},
	{"card", hasPaymentCard},
	{"phone", hasPhone},
}

// screenedText is every field of the candidate the screen reads, one per line. The newline keeps a match from running
// across two fields: no pattern below matches a newline.
func screenedText(c SkillCandidate) string {
	fields := make([]string, 0, 4+len(c.TriggerKeywords))
	fields = append(fields, c.Name, c.Description, c.Content)
	fields = append(fields, c.TriggerKeywords...)
	fields = append(fields, c.Reason)
	return strings.Join(fields, "\n")
}

// emailPattern is an e-mail address in an RFC-lite shape: a local part, an @, dot-separated domain labels and a
// letters-only top-level domain. A version such as v1.2.3 after an @ has no letters-only top-level domain, so it is not
// an address.
var emailPattern = regexp.MustCompile(`[A-Za-z0-9._%+\-]+@[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.[A-Za-z]{2,}\b`)

func hasEmail(text string) bool {
	return emailPattern.MatchString(text)
}

// ibanPattern is an IBAN in its compact form: two letters, two digits and 11 to 30 letters or digits. The printed form,
// with its groups apart, is read by printedIBANInRun.
var ibanPattern = regexp.MustCompile(`(?i)\b[a-z]{2}[0-9]{2}[a-z0-9]{11,30}\b`)

// wordRunPattern is a run of letters and digits joined by spaces or hyphens. The words of a run are the candidate groups
// of a printed IBAN.
var wordRunPattern = regexp.MustCompile(`(?i)[a-z0-9]+(?:[ -]+[a-z0-9]+)*`)

// countryAndCheckPattern is the first group of an IBAN, printed or compact: a country code and two check digits.
var countryAndCheckPattern = regexp.MustCompile(`(?i)^[a-z]{2}[0-9]{2}$`)

// hasIBAN reports whether an IBAN, compact or printed in groups, passes the mod-97 check, which almost no other word of
// this shape does.
func hasIBAN(text string) bool {
	for _, candidate := range ibanPattern.FindAllString(text, -1) {
		if ibanValid(candidate) {
			return true
		}
	}
	for _, run := range wordRunPattern.FindAllString(text, -1) {
		if printedIBANInRun(run) {
			return true
		}
	}
	return false
}

// printedIBANInRun reports whether a run of words holds a printed IBAN: a country code and check digits, then groups of
// four characters, the last group one to four. Every word that starts a candidate is tried, and every prefix of groups is
// tested, so a short word after an IBAN is not read as part of it, and a second IBAN after a first that fails is still
// read.
func printedIBANInRun(run string) bool {
	words := strings.FieldsFunc(run, isGroupSeparator)
	for start := range words {
		if !countryAndCheckPattern.MatchString(words[start]) {
			continue
		}
		compact := ""
		for i := start; i < len(words); i++ {
			if len(words[i]) > 4 || (i > start && len(words[i-1]) != 4) {
				break
			}
			compact += words[i]
			if len(compact) > 34 {
				break
			}
			if len(compact) >= 15 && ibanValid(compact) {
				return true
			}
		}
	}
	return false
}

// ibanValid reports whether an IBAN passes the ISO 7064 mod-97 check. The first four characters move to the end, each
// letter becomes the number from 10 (A) to 35 (Z), and the whole must be 1 modulo 97. The remainder is kept as the
// digits go by, so no large number is built.
func ibanValid(iban string) bool {
	rearranged := iban[4:] + iban[:4]
	remainder := 0
	for i := 0; i < len(rearranged); i++ {
		c := rearranged[i]
		switch {
		case c >= '0' && c <= '9':
			remainder = (remainder*10 + int(c-'0')) % 97
		case c >= 'a' && c <= 'z':
			remainder = (remainder*100 + int(c-'a') + 10) % 97
		case c >= 'A' && c <= 'Z':
			remainder = (remainder*100 + int(c-'A') + 10) % 97
		default:
			return false
		}
	}
	return remainder == 1
}

// numberGroupPattern matches a run of digits, possibly split into digit groups by one or more spaces or hyphens, which is
// how a card or an identity number is written. A group ends at any other character, so a number is never read across a
// line break or through a letter.
var numberGroupPattern = regexp.MustCompile(`[0-9]+(?:[ -]+[0-9]+)*`)

// numberGroups returns the digit groups of text. Each group is the list of its digit runs, in order.
func numberGroups(text string) [][]string {
	var groups [][]string
	for _, group := range numberGroupPattern.FindAllString(text, -1) {
		groups = append(groups, strings.FieldsFunc(group, isGroupSeparator))
	}
	return groups
}

func isGroupSeparator(r rune) bool {
	return r == ' ' || r == '-'
}

// hasNationalID reports whether a Norwegian national identity number appears: eleven digits, written whole or in digit
// groups of any size joined by spaces or hyphens (010190 12480, 010 190 12480), whose two mod-11 control digits are right,
// and whose first six digits are a plausible birth date. Every run of whole digit groups that adds up to eleven digits is
// tried, as hasPaymentCard tries every run that adds up to a card length.
func hasNationalID(text string) bool {
	for _, tokens := range numberGroups(text) {
		for start := range tokens {
			digits := ""
			for _, token := range tokens[start:] {
				digits += token
				if len(digits) > 11 {
					break
				}
				if len(digits) == 11 && validNationalID(digits) {
					return true
				}
			}
		}
	}
	return false
}

var (
	// tenthDigitWeights weight the first nine digits of an identity number to give its tenth, the first control digit.
	tenthDigitWeights = []int{3, 7, 6, 1, 8, 9, 4, 5, 2}
	// eleventhDigitWeights weight the first ten digits to give the eleventh, the second control digit.
	eleventhDigitWeights = []int{5, 4, 3, 2, 7, 6, 5, 4, 3, 2}
)

// validNationalID reports whether id, eleven digits, has both control digits right and a plausible birth date.
func validNationalID(id string) bool {
	if len(id) != 11 {
		return false
	}
	if controlDigit(id[:9], tenthDigitWeights) != int(id[9]-'0') {
		return false
	}
	if controlDigit(id[:10], eleventhDigitWeights) != int(id[10]-'0') {
		return false
	}
	return plausibleBirthDate(id[:6])
}

// controlDigit is the mod-11 control digit of digits under weights: 11 less the weighted sum modulo 11, where 11 reads
// as 0. A result of 10 has no digit, so the function returns -1, which no identity number matches.
func controlDigit(digits string, weights []int) int {
	sum := 0
	for i, weight := range weights {
		sum += weight * int(digits[i]-'0')
	}
	switch k := 11 - sum%11; k {
	case 11:
		return 0
	case 10:
		return -1
	default:
		return k
	}
}

// plausibleBirthDate reports whether six digits DDMMYY can be a birth date. A D-number carries the day plus 40, so a day
// above 40 reads as that day less 40. A February 29 is accepted whenever the two-digit year divides by four, which
// includes 00 for 2000, since the number does not carry the century.
func plausibleBirthDate(ddmmyy string) bool {
	day := int(ddmmyy[0]-'0')*10 + int(ddmmyy[1]-'0')
	month := int(ddmmyy[2]-'0')*10 + int(ddmmyy[3]-'0')
	year := int(ddmmyy[4]-'0')*10 + int(ddmmyy[5]-'0')
	if day > 40 {
		day -= 40
	}
	return month >= 1 && month <= 12 && day >= 1 && day <= daysInMonth(month, year)
}

// daysInMonth is the number of days in a month, for a year given by its last two digits.
func daysInMonth(month, year int) int {
	switch month {
	case 4, 6, 9, 11:
		return 30
	case 2:
		if year%4 == 0 {
			return 29
		}
		return 28
	default:
		return 31
	}
}

// The shortest and longest payment card numbers in use.
const (
	minCardDigits = 13
	maxCardDigits = 19
)

// hasPaymentCard reports whether some run of whole digit runs, taken in order, has 13 to 19 digits and passes the Luhn
// check. Every such run within a group is tried, so a card is found inside a longer line of numbers.
func hasPaymentCard(text string) bool {
	for _, tokens := range numberGroups(text) {
		for start := range tokens {
			digits := ""
			for _, token := range tokens[start:] {
				digits += token
				if len(digits) > maxCardDigits {
					break
				}
				if len(digits) >= minCardDigits && luhnValid(digits) {
					return true
				}
			}
		}
	}
	return false
}

// luhnValid reports whether a string of digits passes the Luhn check: counting from the right, every second digit is
// doubled, and a doubled digit above nine has nine taken off, and the sum must be a multiple of ten.
func luhnValid(digits string) bool {
	sum := 0
	double := false
	for i := len(digits) - 1; i >= 0; i-- {
		d := int(digits[i] - '0')
		if double {
			d *= 2
			if d > 9 {
				d -= 9
			}
		}
		sum += d
		double = !double
	}
	return sum%10 == 0
}

// phonePattern is an international number: a plus sign, then eight to fifteen digits, with optional single spaces
// between them.
var phonePattern = regexp.MustCompile(`\+[0-9](?: ?[0-9]){7,14}`)

// hasPhone reports whether an international number appears. A longer run of digits after the plus sign is not a number
// this screen reads, so such a match is skipped.
func hasPhone(text string) bool {
	for _, location := range phonePattern.FindAllStringIndex(text, -1) {
		if location[1] < len(text) && isASCIIDigit(text[location[1]]) {
			continue
		}
		return true
	}
	return false
}

func isASCIIDigit(c byte) bool {
	return c >= '0' && c <= '9'
}

// secretPatterns are the credential shapes the screen refuses: an AWS access key id, an "sk-" API token, a JWT-like
// token, a PEM private key header and an HTTP bearer credential. A word boundary guards the shapes a longer word could
// otherwise carry.
var secretPatterns = []*regexp.Regexp{
	regexp.MustCompile(`\bAKIA[0-9A-Z]{16}\b`),
	regexp.MustCompile(`\bsk-[A-Za-z0-9_-]{20,}`),
	regexp.MustCompile(`\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*`),
	regexp.MustCompile(`-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----`),
	regexp.MustCompile(`(?i)\bBearer [A-Za-z0-9._~+/=-]{20,}`),
}

func hasSecret(text string) bool {
	for _, pattern := range secretPatterns {
		if pattern.MatchString(text) {
			return true
		}
	}
	return false
}
