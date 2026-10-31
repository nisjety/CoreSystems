package textsafe

import (
	"fmt"
	"testing"
	"unicode"
)

func TestInvisibleIsWhatIsNotDrawn(t *testing.T) {
	t.Parallel()

	for _, r := range []rune{
		// what a list of a few named
		0x00ad, 0x200b, 0x200c, 0x200d, 0xfeff, 0x2060, 0x202a, 0x202b, 0x202c, 0x202d, 0x202e, 0x2066, 0x2067, 0x2068, 0x2069,
		// the marks of the direction
		0x200e, 0x200f, 0x061c,
		// the invisible operators
		0x2061, 0x2062, 0x2063, 0x2064,
		// the old Mongolian vowel separator and the variation selectors, among them the one of the emoji
		0x180e, 0x180b, 0xfe00, 0xfe0f, 0xe0100, 0xe01ef,
		// the tag characters, which can spell out a message no one sees
		0xe0001, 0xe0020, 0xe0041, 0xe007f,
		// the fillers and the grapheme joiner
		0x034f, 0x115f, 0x1160, 0x17b4, 0x17b5, 0x3164, 0xffa0,
		// the boundaries of what is looked up
		0x00ad,
	} {
		if !Invisible(r) {
			t.Errorf("U+%04X is not drawn and is not taken for invisible", r)
		}
	}
}

func TestWhatIsDrawnIsNotInvisible(t *testing.T) {
	t.Parallel()

	for _, r := range []rune{
		'a', 'Z', '0', ' ', '\n', '\t', 'æ', 'ø', 'å', 'Æ', 'Ø', 'Å', 'é', 'ß', 'ǆ',
		0x00ac, 0x00ae, // the neighbours of the soft hyphen
		0x0301, 0x0308, 0x030a, // accents, which show
		0x00a0, 0x2003, 0x2028, 0x2029, 0x3000, // spaces and separators: whitespace, and not what this is for
		0x2010, 0x2013, 0x20ac, 0x2122,
		'한', 'م', '漢', 'あ', 'अ', 0x1f600, 0x0e01,
	} {
		if Invisible(r) {
			t.Errorf("U+%04X is drawn and is taken for invisible", r)
		}
	}
}

// What is called invisible is never a letter, a number, a punctuation mark, a symbol, a space or a control, but for the
// few fillers that are letters in name only: so that nothing that is read is thrown away on the strength of a table.
func TestNothingThatIsReadIsTakenForInvisible(t *testing.T) {
	t.Parallel()

	fillers := map[rune]bool{0x115f: true, 0x1160: true, 0x3164: true, 0xffa0: true}
	var bad []string
	for r := rune(0); r <= unicode.MaxRune; r++ {
		if !Invisible(r) || fillers[r] {
			continue
		}
		if unicode.IsLetter(r) || unicode.IsNumber(r) || unicode.IsPunct(r) || unicode.IsSymbol(r) || unicode.IsSpace(r) || unicode.IsControl(r) {
			bad = append(bad, fmt.Sprintf("U+%04X", r))
		}
	}
	if len(bad) > 0 {
		t.Errorf("taken for invisible, and read: %v", bad)
	}
}

// Everything the standard calls a format character is in it, wherever it is: the table is the check on the few that
// are named above.
func TestEveryFormatCharacterIsInvisible(t *testing.T) {
	t.Parallel()

	for r := rune(0); r <= unicode.MaxRune; r++ {
		if unicode.Is(unicode.Cf, r) && !Invisible(r) {
			t.Errorf("U+%04X is a format character and is not taken for invisible", r)
		}
	}
}
