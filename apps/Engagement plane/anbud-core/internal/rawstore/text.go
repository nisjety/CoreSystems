package rawstore

import (
	"strings"
	"unicode"
)

// cleanText makes s safe for a text column with a length limit. Invalid UTF-8
// and non-printable characters, NUL included, are dropped, and the result is
// clipped to limit characters. A hostile header or message therefore cannot fail
// an insert, and the stored text is never the raw input.
func cleanText(s string, limit int) string {
	s = strings.ToValidUTF8(s, "")
	var b strings.Builder
	kept := 0
	for _, r := range s {
		if !unicode.IsPrint(r) {
			continue
		}
		if kept == limit {
			break
		}
		b.WriteRune(r)
		kept++
	}
	return b.String()
}
