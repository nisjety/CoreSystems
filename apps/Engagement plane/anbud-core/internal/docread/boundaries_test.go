package docread

import (
	"bytes"
	"context"
	"errors"
	"slices"
	"strings"
	"testing"
)

// These tests pin the edges: the byte at which a header is no longer a header, the character at which a
// text is cut, what cleaning takes out of a text. Each is a way the reader could change without a test of
// the ordinary case noticing.

func TestAPDFIsRecognisedByAHeaderThatOpensTheFile(t *testing.T) {
	t.Parallel()

	const rest = "-1.4\nrest of the file"
	for name, tc := range map[string]struct {
		data []byte
		want Kind
	}{
		"the header first":                  {[]byte("%PDF" + rest), KindPDF},
		"a byte order mark before it":       {[]byte("\xef\xbb\xbf%PDF" + rest), KindPDF},
		"blanks and a NUL before it":        {[]byte(" \t\r\n\x00%PDF" + rest), KindPDF},
		"the header inside the first KiB":   {[]byte(strings.Repeat("\n", 1019) + "%PDF" + rest), KindPDF},
		"the header past the first KiB":     {[]byte(strings.Repeat("\n", 1020) + "%PDF" + rest), KindText},
		"the header far past the first KiB": {[]byte(strings.Repeat("\n", 1500) + "%PDF" + rest), KindText},
		"text before the header":            {[]byte("see the header %PDF" + rest), KindText},
	} {
		if got := Detect(tc.data); got != tc.want {
			t.Errorf("%s: Detect = %q, want %q", name, got, tc.want)
		}
	}
}

func TestTextWithAControlCharacterIsNotText(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		data string
		want Kind
	}{
		"a NUL":                   {"abc\x00def", KindOther},
		"a bell":                  {"abc\x07def", KindOther},
		"an escape":               {"abc\x1bdef", KindOther},
		"a delete":                {"abc\x7fdef", KindOther},
		"a tab and line breaks":   {"abc\tdef\r\nghi\n\x0cjkl", KindText},
		"a NUL past the sample":   {strings.Repeat("a", 8192) + "\x00", KindText},
		"a NUL inside the sample": {strings.Repeat("a", 8191) + "\x00", KindOther},
	} {
		if got := Detect([]byte(tc.data)); got != tc.want {
			t.Errorf("%s: Detect = %q, want %q", name, got, tc.want)
		}
	}
}

func TestCapUnitsCutsAtTheLimitExactly(t *testing.T) {
	t.Parallel()

	fresh := func() Content {
		return Content{Units: []Unit{{Page: 1, Text: "abcde"}, {Page: 2, Text: "fghij"}, {Page: 3, Text: "klmno"}, {Page: 4, Text: "pqrst"}}}
	}
	for name, tc := range map[string]struct {
		max       int
		want      []string
		chars     int
		truncated bool
	}{
		"room for all but the last":     {15, []string{"abcde", "fghij", "klmno"}, 15, true}, // the fourth does not fit
		"a limit that falls in a unit":  {12, []string{"abcde", "fghij", "kl"}, 12, true},
		"a limit one short of a unit":   {14, []string{"abcde", "fghij", "klmn"}, 14, true},
		"a limit at the end of a unit":  {10, []string{"abcde", "fghij"}, 10, true},
		"a limit of one":                {1, []string{"a"}, 1, true},
		"a limit of nothing":            {0, nil, 0, true},
		"room for everything":           {20, []string{"abcde", "fghij", "klmno", "pqrst"}, 20, false},
		"room for more than everything": {99, []string{"abcde", "fghij", "klmno", "pqrst"}, 20, false},
	} {
		c := fresh()
		capUnits(&c, tc.max)
		if got := texts(c.Units); !slices.Equal(got, tc.want) || c.Chars != tc.chars || c.Truncated != tc.truncated {
			t.Errorf("%s: units %q chars %d truncated %v; want %q %d %v", name, got, c.Chars, c.Truncated, tc.want, tc.chars, tc.truncated)
		}
		if len(c.Units) > 0 && c.Units[0].Page != 1 {
			t.Errorf("%s: the units lost their pages: %+v", name, c.Units)
		}
	}
}

// The builder is the same cut, made as the paragraphs come, and it stops asking for more once it is full.
func TestUnitsAreCutAtTheLimitAsTheParagraphsComeAndNumberedFromOne(t *testing.T) {
	t.Parallel()

	paragraphs := []string{"abcde", "", "  ", "fghij", "\n", "klmno", "pqrst"}
	for max, want := range map[int][]string{
		15: {"abcde", "fghij", "klmno"},
		12: {"abcde", "fghij", "kl"},
		5:  {"abcde"},
		20: {"abcde", "fghij", "klmno", "pqrst"},
	} {
		c := unitsOf(KindText, "text", paragraphs, max)
		if got := texts(c.Units); !slices.Equal(got, want) {
			t.Errorf("max %d: units %q, want %q", max, got, want)
		}
		for i, u := range c.Units {
			if u.Paragraph != i+1 {
				t.Errorf("max %d: unit %d is paragraph %d: the numbers follow the units that were kept, from 1", max, i+1, u.Paragraph)
			}
		}
	}
	// Truncated says there was more, and only then.
	if c := unitsOf(KindText, "text", []string{"abcde", "fghij"}, 10); c.Truncated || c.Chars != 10 {
		t.Errorf("exactly the limit: %+v", c)
	}
	if c := unitsOf(KindText, "text", []string{"abcde", "fghij", "k"}, 10); !c.Truncated {
		t.Errorf("one character over: not truncated")
	}

	b := newBuilder(KindText, "text", 5)
	if !b.add("abc") || b.add("defgh") || b.add("i") {
		t.Errorf("a builder is full after the paragraph that crosses the limit, and stays full")
	}
	if got := texts(b.c.Units); !slices.Equal(got, []string{"abc", "de"}) {
		t.Errorf("units %q", got)
	}
}

// A file of very many short lines costs the limit, not the number of lines.
func TestATextFileOfManyLinesIsReadOnlyAsFarAsTheLimit(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 1000
	c, err := readText([]byte(strings.Repeat("x\n", 5_000_000)), lim)
	if err != nil || c.Chars != 1000 || len(c.Units) != 1000 || !c.Truncated {
		t.Errorf("chars %d units %d truncated %v, %v", c.Chars, len(c.Units), c.Truncated, err)
	}
}

func TestWhatCleaningTakesOutOfText(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct{ in, want string }{
		"a no-break space":          {"a\xc2\xa0b", "a b"},
		"a wide space":              {"a\xe2\x80\x83b", "a b"},
		"a tab":                     {"a\tb", "a b"},
		"a soft hyphen":             {"a\xc2\xadb", "ab"},
		"a zero-width space":        {"a\xe2\x80\x8bb", "ab"},
		"a zero-width non-joiner":   {"a\xe2\x80\x8cb", "ab"},
		"a zero-width joiner":       {"a\xe2\x80\x8db", "ab"},
		"a byte order mark":         {"a\xef\xbb\xbfb", "ab"},
		"a word joiner":             {"a\xe2\x81\xa0b", "ab"},
		"a left-to-right embedding": {"a\xe2\x80\xaab", "ab"},
		"a right-to-left override":  {"a\xe2\x80\xaeb", "ab"},
		"a bidirectional isolate":   {"a\xe2\x81\xa6b\xe2\x81\xa9", "ab"},
		// What cannot be seen is not kept because the list of a few did not name it: the marks that say which way a
		// line reads, the invisible operators, the tag characters (a message no one sees), the variation selectors and
		// the fillers. A sentence kept from a tender is shown to a person, and may be read by something else.
		"a left-to-right mark":         {"a\u200eb", "ab"},
		"a right-to-left mark":         {"a\u200fb", "ab"},
		"an Arabic letter mark":        {"a\u061cb", "ab"},
		"an invisible function sign":   {"a\u2061b\u2062c\u2063d\u2064e", "abcde"},
		"a Mongolian vowel separator":  {"a\u180eb", "ab"},
		"a tag character":              {"a\U000E0041\U000E0042b", "ab"},
		"a variation selector":         {"a\ufe0fb\U000E0100c", "abc"},
		"a combining grapheme joiner":  {"a\u034fb", "ab"},
		"a Hangul filler":              {"a\u3164b\uffa0c\u115fd\u1160e", "abcde"},
		"a Khmer inherent vowel":       {"a\u17b4b\u17b5c", "abc"},
		"letters of other scripts":     {"한글 مرحبا 漢字 😀", "한글 مرحبا 漢字 😀"},
		"a NUL":                        {"a\x00b", "ab"},
		"a bell":                       {"a\x07b", "ab"},
		"a replacement character":      {"a\xef\xbf\xbdb", "ab"},
		"runs of blanks":               {"a   b \t c", "a b c"},
		"a Windows line break":         {"a\r\nb", "a\nb"},
		"an old Mac line break":        {"a\rb", "a\nb"},
		"several empty lines":          {"a\n\n\n\nb", "a\n\nb"},
		"blanks at the ends of lines":  {"a  \n  b", "a\nb"},
		"blanks at the ends of a text": {"  a  ", "a"},
		"a decomposed letter":          {"e\xcc\x81", "\xc3\xa9"},
		"nothing":                      {" \t\n ", ""},
		"Norwegian letters":            {"æøå ÆØÅ", "æøå ÆØÅ"},
	} {
		if got := cleanText(tc.in); got != tc.want {
			t.Errorf("%s: cleanText(%q) = %q, want %q", name, tc.in, got, tc.want)
		}
	}
}

// A runner that records what it is asked, for the tests of what the reader gives a PDF reader.
type recordingRunner struct {
	calls    int
	maxPages int
	result   PDFResult
	err      error
}

func (r *recordingRunner) Run(_ context.Context, _ []byte, maxPages int) (PDFResult, error) {
	r.calls++
	r.maxPages = maxPages
	return r.result, r.err
}

func TestAPDFAtTheByteLimitIsReadAndOneByteOverIsNot(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxPDFBytes = 20
	lim.MaxPDFPages = 7
	run := &recordingRunner{result: PDFResult{Pages: []string{"en"}, TotalPages: 1}}
	r := NewReader(run, lim)

	if _, err := r.Read(context.Background(), []byte("%PDF-1.4"+strings.Repeat("x", 12))); err != nil || run.calls != 1 {
		t.Errorf("a PDF of exactly the limit: %v after %d calls", err, run.calls)
	}
	if run.maxPages != 7 {
		t.Errorf("the reader was told to read %d pages, want the limit of 7", run.maxPages)
	}
	if _, err := r.Read(context.Background(), []byte("%PDF-1.4"+strings.Repeat("x", 13))); !errors.Is(err, ErrTooLarge) || run.calls != 1 {
		t.Errorf("a PDF one byte over: %v after %d calls, want ErrTooLarge and no call", err, run.calls)
	}
}

func TestACancelledContextReadsNothingOfAnyKind(t *testing.T) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	for name, data := range map[string][]byte{
		"text": []byte("Leverandøren skal ha ISO 9001."),
		"html": []byte("<html><body><p>ord</p></body></html>"),
		"docx": docxFile(t, para("ord")),
	} {
		c, err := NewReader(&recordingRunner{}, DefaultLimits).Read(ctx, data)
		if !errors.Is(err, context.Canceled) || len(c.Units) != 0 {
			t.Errorf("%s: %d units, %v; want nothing read and the context's error", name, len(c.Units), err)
		}
	}
}

func TestAPDFOfNothingOrOfNoPagesIsDamagedAndSaysWhich(t *testing.T) {
	t.Parallel()

	if _, err := ExtractPDF(nil, 10); !errors.Is(err, ErrMalformed) || !strings.Contains(err.Error(), "empty") {
		t.Errorf("an empty file: %v", err)
	}
	if _, err := collectPages(0, 10, nil); !errors.Is(err, ErrMalformed) || !strings.Contains(err.Error(), "no pages") {
		t.Errorf("a file of no pages: %v", err)
	}
}

// A page that cannot be read is an empty page, and the file is damaged only if no page could be read.
func TestAPageThatCannotBeReadIsAnEmptyPageAndNotTheFilesFailure(t *testing.T) {
	t.Parallel()

	pages := map[int]string{1: "en", 3: "tre"}
	res, err := collectPages(3, 10, func(n int) (string, bool) {
		text, ok := pages[n]
		return text, ok
	})
	if err != nil || !slices.Equal(res.Pages, []string{"en", "", "tre"}) || res.TotalPages != 3 || res.Truncated {
		t.Errorf("one page failed of three: %+v, %v", res, err)
	}
	if _, err := collectPages(3, 10, func(int) (string, bool) { return "", false }); !errors.Is(err, ErrMalformed) {
		t.Errorf("every page failed: %v, want ErrMalformed", err)
	}
	// A page with no text that could be read is not a failure: the file is a scan, not damaged.
	if res, err := collectPages(2, 10, func(int) (string, bool) { return "", true }); err != nil || len(res.Pages) != 2 {
		t.Errorf("pages with no text: %+v, %v", res, err)
	}
	// Only the pages within the limit are read, and the answer says there were more.
	var asked []int
	res, err = collectPages(5, 2, func(n int) (string, bool) { asked = append(asked, n); return "x", true })
	if err != nil || !slices.Equal(asked, []int{1, 2}) || !res.Truncated || res.TotalPages != 5 {
		t.Errorf("a limit of two of five: asked %v, %+v, %v", asked, res, err)
	}
	if res, _ := collectPages(5, 0, func(int) (string, bool) { return "x", true }); len(res.Pages) != 5 || res.Truncated {
		t.Errorf("no limit: %+v", res)
	}
}

func TestAPageThatPanicsOrFailsIsThatPagesFailure(t *testing.T) {
	t.Parallel()

	if text, ok := safePage(func() (string, error) { panic("the library gave up") }); ok || text != "" {
		t.Errorf("a panic: %q, %v", text, ok)
	}
	if text, ok := safePage(func() (string, error) { return "halv", errors.New("a failure") }); ok || text != "" {
		t.Errorf("an error: %q, %v: what came with an error is not text", text, ok)
	}
	if text, ok := safePage(func() (string, error) { return "hel", nil }); !ok || text != "hel" {
		t.Errorf("a page: %q, %v", text, ok)
	}
}

func TestAWindows1252FileIsDecodedAndAUTF8FileIsNot(t *testing.T) {
	t.Parallel()

	if c, _ := readText([]byte("F\xf8rste \xe6re \xe5r"), DefaultLimits); texts(c.Units)[0] != "Første ære år" {
		t.Errorf("Windows-1252: %q", texts(c.Units))
	}
	if c, _ := readText([]byte("Første ære år"), DefaultLimits); texts(c.Units)[0] != "Første ære år" {
		t.Errorf("UTF-8: %q", texts(c.Units))
	}
	if c, _ := readText(append([]byte{0xFF, 0xFE}, []byte("h\x00i\x00")...), DefaultLimits); texts(c.Units)[0] != "hi" {
		t.Errorf("UTF-16 little endian: %q", texts(c.Units))
	}
	if c, _ := readText(append([]byte{0xFE, 0xFF}, []byte("\x00h\x00i")...), DefaultLimits); texts(c.Units)[0] != "hi" {
		t.Errorf("UTF-16 big endian: %q", texts(c.Units))
	}
	if c, _ := readText(append([]byte{0xEF, 0xBB, 0xBF}, []byte("hei")...), DefaultLimits); texts(c.Units)[0] != "hei" {
		t.Errorf("UTF-8 with a mark: %q", texts(c.Units))
	}
	// The mark is taken off before the text is judged: a file that begins with one and is not UTF-8 is Windows-1252
	// all the same, and the mark is not read as three letters of it.
	if c, _ := readText(append([]byte{0xEF, 0xBB, 0xBF}, []byte("F\xf8rste")...), DefaultLimits); texts(c.Units)[0] != "Første" {
		t.Errorf("a UTF-8 mark and then Windows-1252: %q", texts(c.Units))
	}
}

// bytes is used by the tests above only through helpers.
var _ = bytes.NewReader

// unitsOf makes a unit of each non-empty paragraph, numbered from 1, and stops at max characters: what the readers
// do with the paragraphs they find, through the builder.
func unitsOf(kind Kind, extractor string, paragraphs []string, max int) Content {
	b := newBuilder(kind, extractor, max)
	for _, p := range paragraphs {
		if !b.add(p) {
			break
		}
	}
	return b.c
}
