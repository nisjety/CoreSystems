package match

import (
	"regexp"
	"strings"
	"unicode"
	"unicode/utf8"
)

// maxSnippetRunes is the longest passage a finding quotes.
const maxSnippetRunes = 320

// The cues the finder reads a sentence by. They are Norwegian and English, lower case, and matched
// as substrings. They are placeholders in the same sense as the requirement rules: the owner's list
// to replace, not a classification anyone has checked against outcomes.
var (
	// negationCues say the capability is not asked for.
	negationCues = []string{
		"trenger ikke", "behøver ikke", "kreves ikke", "ikke krav", "ikke et krav", "stilles ikke krav",
		"ikke påkrevd", "ikke nødvendig", "ingen krav", "uten krav",
		"not required", "no requirement", "does not require", "is not a requirement",
	}
	// qualificationCues say the sentence states a condition for taking part, whatever else it says.
	qualificationCues = []string{"kvalifikasjon", "minstekrav", "qualification"}
	// awardCues say the sentence is about scoring tenders.
	awardCues = []string{"tildelingskriteri", "uttelling", "poeng", "vektes", "vektet", "evaluering", "scoring", "award criteri"}
	// requirementCues say something is asked for.
	requirementCues = []string{
		" skal ", " må ", "kreves", "påkrevd", "forutsetter", "obligatorisk", "krav til", "krav om", "krav:",
		"must", "shall", "required", "mandatory",
	}
)

// FindInPassages looks in the text of one document for sentences that name a capability the
// requirement rules know, and returns one finding for each capability each sentence names. The
// sentence is judged alone, by the cues above: negated before anything, then a qualification cue,
// then an award cue, then a requirement cue; a sentence with none only mentions it. A list item takes
// the class of the lead-in above it ("shall document the following:") unless it has a cue of its own.
func FindInPassages(document string, passages []Passage) []Finding {
	var out []Finding
	for _, ps := range passages {
		var inherited FindingClass // the class of the lead-in of the list we are in
		for _, para := range paragraphsOf(ps.Text) {
			last := ClassMention // the class of the paragraph's last sentence
			for _, sentence := range splitSentences(para.text) {
				lower := strings.ToLower(sentence)
				class := classify(lower)
				if class == ClassMention && para.item && inherited != "" {
					class = inherited
				}
				for _, rule := range DefaultRequirementRules {
					if at := firstKeyword(lower, rule.Keywords); at >= 0 {
						out = append(out, Finding{
							Capability: rule.Capability, Class: class, Document: document,
							Page: ps.Page, Paragraph: ps.Paragraph, Snippet: snippetAround(sentence, lower, at),
						})
					}
				}
				last = class
			}
			switch {
			case strings.HasSuffix(para.text, ":") && last != ClassMention:
				inherited = last // a lead-in: the items under it are what it says they are
			case !para.item:
				inherited = "" // a paragraph that is not an item ends the list
			}
		}
	}
	return out
}

// classify judges one sentence, lower case.
func classify(lower string) FindingClass {
	switch {
	case containsAny(lower, negationCues):
		return ClassNegated
	case containsAny(lower, qualificationCues):
		return ClassRequirement
	case containsAny(lower, awardCues):
		return ClassAward
	case containsAny(" "+lower+" ", requirementCues):
		return ClassRequirement
	}
	return ClassMention
}

func containsAny(s string, cues []string) bool {
	for _, c := range cues {
		if strings.Contains(s, c) {
			return true
		}
	}
	return false
}

// firstKeyword is the byte index of the earliest keyword in lower, or -1.
func firstKeyword(lower string, keywords []string) int {
	best := -1
	for _, k := range keywords {
		if i := strings.Index(lower, k); i >= 0 && (best < 0 || i < best) {
			best = i
		}
	}
	return best
}

// snippetAround quotes the sentence, or, when it is longer than a snippet may be, the stretch of it
// around the keyword, cut at word boundaries and marked with an ellipsis where it is cut.
func snippetAround(sentence, lower string, keywordAt int) string {
	runes := []rune(sentence)
	if len(runes) <= maxSnippetRunes {
		return sentence
	}
	centre := utf8.RuneCountInString(lower[:keywordAt])
	if centre >= len(runes) {
		centre = len(runes) - 1
	}
	start := centre - maxSnippetRunes/2
	if start < 0 {
		start = 0
	}
	end := start + maxSnippetRunes
	if end > len(runes) {
		end = len(runes)
		if start = end - maxSnippetRunes; start < 0 {
			start = 0
		}
	}
	for start > 0 && start < centre && !unicode.IsSpace(runes[start-1]) {
		start++ // forward, to the start of a word
	}
	for end < len(runes) && end > centre && !unicode.IsSpace(runes[end]) {
		end-- // back, to the end of a word
	}
	out := strings.TrimSpace(string(runes[start:end]))
	if start > 0 {
		out = "…" + out
	}
	if end < len(runes) {
		out += "…"
	}
	return out
}

// paragraph is text that belongs together, and whether it is an item of a list.
type paragraph struct {
	text string
	item bool
}

var (
	// listMarker starts a line that is an item of a list, or a clause with a number of its own.
	listMarker = regexp.MustCompile(`^(?:[•◦▪·*\-–—]|\d{1,3}[.)]|\([a-zA-Z0-9]\)|[a-z][.)]|\d{1,2}(?:\.\d{1,2})+)\s+`)
	// bulletMarker is the part of a marker that is only a bullet, and is not quoted.
	bulletMarker = regexp.MustCompile(`^(?:[•◦▪·*\-–—]|\d{1,3}[.)]|\([a-zA-Z0-9]\)|[a-z][.)])\s+`)
)

// paragraphsOf rebuilds the paragraphs of a page's text. A PDF has a line break wherever its page
// wraps, so lines are joined, except where a line ends a sentence, or is short (under 60 per cent of
// the longest line of the page) and the next begins a new one, or the next is an item of a list.
// A word split at a hyphen is joined: "Miljøfyrtårn-" and "sertifisering" are one word.
func paragraphsOf(text string) []paragraph {
	lines := strings.Split(text, "\n")
	trimmed := make([]string, len(lines))
	longest := 0
	for i, l := range lines {
		trimmed[i] = strings.TrimSpace(l)
		if n := utf8.RuneCountInString(trimmed[i]); n > longest {
			longest = n
		}
	}

	var out []paragraph
	var cur strings.Builder
	item := false
	flush := func() {
		if s := strings.TrimSpace(cur.String()); s != "" {
			out = append(out, paragraph{text: s, item: item})
		}
		cur.Reset()
		item = false
	}

	for i, line := range trimmed {
		if line == "" {
			flush()
			continue
		}
		if listMarker.MatchString(line) {
			flush()
			if bulletMarker.MatchString(line) {
				item = true
				line = bulletMarker.ReplaceAllString(line, "")
			}
		}
		if cur.Len() > 0 {
			if strings.HasSuffix(cur.String(), "-") && startsLower(line) {
				// joined without a space: the hyphen stays, as it is part of the compound
			} else {
				cur.WriteByte(' ')
			}
		}
		cur.WriteString(line)

		// A line that nearly fills the page wraps into the next. A shorter one ends its paragraph when
		// the next line begins a new sentence, and ends its item, which is short as a rule.
		full := longest >= 40 && utf8.RuneCountInString(line)*10 >= longest*6
		switch {
		case endsSentence(line), nextIsItem(trimmed, i):
			flush()
		case item && !full:
			flush()
		case !item && !full && longest >= 40 && nextStartsNew(trimmed, i):
			flush()
		}
	}
	flush()
	return out
}

func startsLower(s string) bool {
	r, _ := utf8.DecodeRuneInString(s)
	return unicode.IsLower(r)
}

// nextStartsNew reports whether the next line begins like a new sentence: with a capital, a digit or a
// marker, and not with a lower case letter that continues this one. A blank line, or none, ends it.
func nextStartsNew(lines []string, i int) bool {
	if i+1 >= len(lines) || lines[i+1] == "" {
		return true
	}
	r, _ := utf8.DecodeRuneInString(lines[i+1])
	return !unicode.IsLower(r)
}

func nextIsItem(lines []string, i int) bool {
	for _, l := range lines[i+1:] {
		if l != "" {
			return listMarker.MatchString(l)
		}
	}
	return false
}

// endsSentence reports whether a line ends with a sentence terminator that is not an abbreviation.
func endsSentence(line string) bool {
	r, size := utf8.DecodeLastRuneInString(line)
	switch r {
	case '!', '?', ';', ':':
		return true
	case '.':
		return !isAbbreviation(line[:len(line)-size])
	}
	return false
}

// abbreviations do not end a sentence. They are Norwegian and English, lower case, without the final dot.
var abbreviations = map[string]bool{
	"bl.a": true, "f.eks": true, "m.fl": true, "m.v": true, "o.l": true, "o.a": true, "ca": true, "pkt": true,
	"nr": true, "jf": true, "mv": true, "osv": true, "dvs": true, "inkl": true, "evt": true, "mfl": true,
	"kr": true, "mill": true, "mrd": true, "kap": true, "ref": true, "ift": true, "iht": true, "etc": true,
	"e.g": true, "i.e": true, "cf": true, "vs": true, "st": true, "art": true, "ev": true, "dr": true, "bla": true,
}

// isAbbreviation reports whether the text before a full stop ends in an abbreviation.
func isAbbreviation(before string) bool {
	if i := strings.LastIndexFunc(before, unicode.IsSpace); i >= 0 {
		before = before[i+1:]
	}
	return abbreviations[strings.ToLower(before)]
}

// lastWord is the text after the last blank in runes, which is all isAbbreviation looks at. Taking it
// directly, and not the whole sentence so far, keeps a long run of abbreviations ("ca. 4 ca. 7 ...") from
// costing the length of the paragraph at each of them.
func lastWord(runes []rune) string {
	j := len(runes)
	for j > 0 && !unicode.IsSpace(runes[j-1]) {
		j--
	}
	return string(runes[j:])
}

// splitSentences cuts a paragraph into sentences at a full stop, an exclamation or question mark or
// a semicolon that is followed by a space and then something that begins a sentence: a capital, a
// digit, a quotation mark or a bracket. A stop inside a number ("3.1") has no space after it, and one
// after an abbreviation ("kap.") is not an end.
func splitSentences(p string) []string {
	runes := []rune(p)
	var out []string
	start := 0
	for i, r := range runes {
		if r != '.' && r != '!' && r != '?' && r != ';' {
			continue
		}
		next := i + 1
		if next < len(runes) && !unicode.IsSpace(runes[next]) {
			continue
		}
		k := next
		for k < len(runes) && unicode.IsSpace(runes[k]) {
			k++
		}
		if k < len(runes) {
			c := runes[k]
			if !(unicode.IsUpper(c) || unicode.IsDigit(c) || c == '"' || c == '«' || c == '(') {
				continue
			}
		}
		if r == '.' && isAbbreviation(lastWord(runes[start:i])) {
			continue
		}
		if s := strings.TrimSpace(string(runes[start : i+1])); s != "" {
			out = append(out, s)
		}
		start = k
	}
	if start < len(runes) {
		if s := strings.TrimSpace(string(runes[start:])); s != "" {
			out = append(out, s)
		}
	}
	return out
}
