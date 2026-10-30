// Package textsafe says what is not kept of a text because it cannot be seen. A name or a sentence from a stranger's
// file is stored, shown to a person and perhaps read by something else, and a character that is not drawn can make it
// say one thing to the person and another to the reader, or read backwards.
package textsafe

import "unicode"

// hidden are the characters of the Unicode standard that are not drawn: the default ignorable code points (the soft
// hyphen, the zero-width characters, the marks and overrides that say which way a line reads, the invisible
// operators, the variation selectors, the tag characters, the fillers and the grapheme joiner) and the other format
// characters. A list of a few that someone thought of was in three places, and none of them had the marks of the
// direction, the invisible operators, the tag characters (which can spell out a message no one sees), the variation
// selectors or the fillers.
var hidden = []*unicode.RangeTable{unicode.Cf, unicode.Variation_Selector, unicode.Other_Default_Ignorable_Code_Point}

// firstHidden is the lowest of them, the soft hyphen: what is below it is never looked up.
const firstHidden = 0x00ad

// Invisible reports whether r is not drawn.
func Invisible(r rune) bool {
	if r < firstHidden {
		return false
	}
	return unicode.In(r, hidden...)
}
