package docread

import (
	"slices"
	"strings"
	"testing"
)

func texts(units []Unit) []string {
	out := make([]string, len(units))
	for i, u := range units {
		out[i] = u.Text
	}
	return out
}

func TestReadTextMakesOneUnitOfEachLine(t *testing.T) {
	t.Parallel()

	c, err := readText([]byte("\xef\xbb\xbfFørste linje\r\n\r\n  Andre   linje med  mellomrom \nTredje"), DefaultLimits)
	if err != nil {
		t.Fatalf("readText: %v", err)
	}
	if got, want := texts(c.Units), []string{"Første linje", "Andre linje med mellomrom", "Tredje"}; !slices.Equal(got, want) {
		t.Errorf("units = %q, want %q", got, want)
	}
	for i, u := range c.Units {
		if u.Paragraph != i+1 || u.Page != 0 {
			t.Errorf("unit %d = %+v, want paragraph %d and no page", i, u, i+1)
		}
	}
}

func TestReadTextDecodesTheEncodingsNorwegianFilesComeIn(t *testing.T) {
	t.Parallel()

	utf16le := append([]byte{0xFF, 0xFE}, []byte{'B', 0, 'l', 0, 0xE5, 0, 'b', 0, 0xE6, 0, 'r', 0}...)
	utf16be := append([]byte{0xFE, 0xFF}, []byte{0, 'B', 0, 'l', 0, 0xE5, 0, 'b', 0, 0xE6, 0, 'r'}...)
	for name, data := range map[string][]byte{
		"utf-8":        []byte("Blåbær"),
		"windows-1252": []byte("Bl\xe5b\xe6r"),
		"utf-16 le":    utf16le,
		"utf-16 be":    utf16be,
	} {
		c, err := readText(data, DefaultLimits)
		if err != nil || len(c.Units) != 1 || c.Units[0].Text != "Blåbær" {
			t.Errorf("%s: %q, %v; want Blåbær", name, texts(c.Units), err)
		}
	}
}

func TestCleanTextRemovesWhatIsNotText(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct{ in, want string }{
		"no-break space":     {"a\xc2\xa0b", "a b"},
		"soft hyphen":        {"mil\xc2\xadjø", "miljø"},
		"zero width":         {"a\xe2\x80\x8bb\xe2\x80\x8c\xef\xbb\xbfc", "abc"},
		"control characters": {"a\x00b\x07c\x1bd", "abcd"},
		"tab":                {"a\tb", "a b"},
		"runs of spaces":     {"a    b", "a b"},
		"crlf":               {"a\r\nb\rc", "a\nb\nc"},
		"blank lines":        {"a\n\n\n\nb", "a\n\nb"},
		"around newlines":    {"a \n b", "a\nb"},
		"decomposed":         {"a\xcc\x8a", "å"},
		"bidi override":      {"a\xe2\x80\xaeb", "ab"},
		"trimmed":            {"  \n a \n  ", "a"},
	} {
		if got := cleanText(tc.in); got != tc.want {
			t.Errorf("%s: cleanText(%q) = %q, want %q", name, tc.in, got, tc.want)
		}
	}
}

func TestReadTextKeepsTheLimit(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 20
	c, err := readText([]byte(strings.Repeat("abcdefghij\n", 10)), lim)
	if err != nil {
		t.Fatalf("readText: %v", err)
	}
	if c.Chars > lim.MaxChars || !c.Truncated {
		t.Errorf("chars %d truncated %v, want at most %d and truncated", c.Chars, c.Truncated, lim.MaxChars)
	}
}
