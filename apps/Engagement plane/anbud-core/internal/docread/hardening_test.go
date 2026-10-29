package docread

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/binary"
	"encoding/xml"
	"errors"
	"fmt"
	"golang.org/x/text/unicode/norm"
	"io"
	"math/rand"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode"
	utf16pkg "unicode/utf16"
	"unicode/utf8"
)

// ---------------------------------------------------------------- what a hostile file must not cost

// wordDocument is a DOCX whose main part is the given XML.
func wordDocument(t *testing.T, xml string) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for name, data := range map[string]string{
		"[Content_Types].xml": `<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"></Types>`,
		"word/document.xml":   xml,
	} {
		w, err := zw.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = w.Write([]byte(data))
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

const wordOpen = `<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>`
const wordClose = `</w:body></w:document>`

func spent(f func()) uint64 {
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	f()
	runtime.ReadMemStats(&after)
	return after.TotalAlloc - before.TotalAlloc
}

// A tag of four million attributes costs the decoder half a gigabyte from nineteen megabytes of XML. A tag past
// the limit is refused as it is read, and nothing is held for it. Not parallel: the figure is the process's.
//
// A scanner of its own for where a tag ends is led out of step by what comes before it: a quote in a comment makes
// it take the first ">" in a value for the end of the tag, and the millions of attributes after it for text. The
// meter counts what the decoder takes and is led by nothing, and each of these is refused alike.
func TestATagOfManyAttributesIsRefusedBeforeItIsHeld(t *testing.T) {
	attributes := map[string]string{
		"attributes":             ` a="1"`,
		"namespace declarations": ` xmlns:n="u"`,
		"a > in every value":     ` a=">"`, // the end of the tag is not the first ">"
		"single quotes":          ` a='>'`,
	}
	before := map[string]string{
		"nothing before it":                ``,
		"a stray quote in a comment":       `<!-- " -->`,
		"a stray apostrophe in a comment":  `<!-- ' -->`,
		"a stray quote in a CDATA section": `<w:p><w:r><w:t><![CDATA[ " ]]></w:t></w:r></w:p>`,
		"a stray quote in an instruction":  `<?note " ?>`,
		"a stray quote in a declaration":   `<!DOCTYPE w [ <!ENTITY e "x"> ]>`,
		"quotes and a > in a comment":      `<!-- " > ' > " -->`,
	}
	for attrName, attr := range attributes {
		for beforeName, prefix := range before {
			opening := `<w:p z=">"`
			if attrName != "attributes" && attrName != "a > in every value" {
				opening = `<w:p`
			}
			data := wordDocument(t, wordOpen+prefix+opening+strings.Repeat(attr, 100_000)+`>x</w:p>`+wordClose)
			var err error
			if got := spent(func() { _, err = readDOCX(data, DefaultLimits) }); got > 16<<20 {
				t.Errorf("%s, %s: reading cost %d bytes: the tag was held", attrName, beforeName, got)
			}
			if !errors.Is(err, ErrMalformed) {
				t.Errorf("%s, %s: err = %v, want ErrMalformed", attrName, beforeName, err)
			}
		}
	}
}

// What the limit is on is the tag: a long run of text is text, and tags of ordinary length are read.
func TestALongTextAndTagsOfOrdinaryLengthAreRead(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("ord ", 100_000) // 400 KB of text in one run
	data := wordDocument(t, wordOpen+`<w:p><w:r><w:t>`+long+`</w:t></w:r></w:p>`+
		`<w:p w:a="`+strings.Repeat("x", 30_000)+`"><w:r><w:t>kort</w:t></w:r></w:p>`+wordClose)
	c, err := readDOCX(data, DefaultLimits)
	if err != nil || len(c.Units) != 2 || c.Units[1].Text != "kort" {
		t.Fatalf("units %d, %v", len(c.Units), err)
	}
}

// tokensOf drives a decoder over the input through a meter with the given limits, as the reader of a DOCX does,
// and returns what each token took of the input. The error is the first the decoder met, or nil at the end.
func tokensOf(input string, markup, text int64) (sizes []int64, err error) {
	meter := newXMLMeter(strings.NewReader(input), markup, text)
	d := xml.NewDecoder(meter)
	for {
		meter.begin()
		_, err := d.Token()
		if err != nil {
			if errors.Is(err, io.EOF) {
				return sizes, nil
			}
			return sizes, err
		}
		sizes = append(sizes, meter.used())
	}
}

func TestTheLengthOfATokenOfMarkupIsBoundedExactly(t *testing.T) {
	t.Parallel()

	tooLong := func(input string) bool {
		_, err := tokensOf(input, 10, 1000)
		return errors.Is(err, errXMLTokenTooLong)
	}
	for name, tc := range map[string]struct {
		in      string
		refused bool
	}{
		"a tag of the limit":                        {"<" + strings.Repeat("a", 8) + ">", false}, // ten bytes from the "<" to the ">"
		"a tag one over":                            {"<" + strings.Repeat("a", 9) + ">", true},
		"text outside a tag is not counted as one":  {"<r>" + strings.Repeat("t", 900) + "</r>", false},
		"each tag is counted from its own <":        {"<r>" + strings.Repeat("<aaaaaaaa>", 20) + "</r>", false},
		"each tag after text is counted from its <": {"<r>" + strings.Repeat("t<aaaaaaa>", 20) + "</r>", false},
		"a tag after text, one over":                {"<r>t<" + strings.Repeat("a", 9) + ">t</r>", true},
		"a > in a double-quoted value is no end":    {`<a v=">>>>>>>>>>>>">`, true},
		"a > in a single-quoted value is no end":    {`<a v='>>>>>>>>>>>>'>`, true},
		"a quote of the other kind does not close":  {`<a v="'>>>>>>>>>>>>">`, true},
		"a closed value lets the tag end":           {`<a v="x">` + strings.Repeat("t", 100), false},
		"an unterminated tag is refused":            {"<" + strings.Repeat("a", 100), true},
		"a tag that ends and another that follows":  {"<aaaaaaaa><aaaaaaaa>", false},
		"a quote outside a tag means nothing":       {`<r>"""""""""""""""""""""</r>`, false},
		"an end tag is counted too":                 {"<r>t</" + strings.Repeat("r", 11) + ">", true},
		"a comment is counted":                      {"<r><!--" + strings.Repeat("c", 10) + "--></r>", true},
		"a comment of the limit":                    {"<r><!--ccc--></r>", false},
		"an instruction is counted":                 {"<?" + strings.Repeat("p", 20) + "?><r/>", true},
		"a CDATA section is counted":                {"<r><![CDATA[" + strings.Repeat("c", 10) + "]]></r>", true},
		"a declaration is counted":                  {"<!DOCTYPE " + strings.Repeat("d", 10) + "><r/>", true},
	} {
		if got := tooLong(tc.in); got != tc.refused {
			t.Errorf("%s: refused = %v, want %v", name, got, tc.refused)
		}
	}
}

// What leads a scanner of its own out of step does nothing to the meter: after a comment, a CDATA section, an
// instruction or a declaration that holds a quote, a tag is counted from its "<" to its ">" and a long tag is
// refused, and a short one is not.
func TestWhatIsBeforeATagDoesNotChangeWhatItIsCountedAs(t *testing.T) {
	t.Parallel()

	long := `<w:p z=">"` + strings.Repeat(` a=""`, 20) + `>`
	short := `<w:p z=">">`
	for name, before := range map[string]string{
		"a comment with a quote":         `<!-- " -->`,
		"a comment with an apostrophe":   `<!-- ' -->`,
		"a CDATA section with a quote":   `<r><![CDATA[ " ]]></r>`,
		"an instruction with a quote":    `<?x " ?>`,
		"a declaration with quotes":      `<!DOCTYPE r "x">`,
		"text with a quote":              `<r>"</r>`,
		"a value that ends the tag late": `<q v="a>b">`,
	} {
		if _, err := tokensOf(before+short, 20, 1000); errors.Is(err, errXMLTokenTooLong) {
			t.Errorf("%s: a short tag after it is refused", name)
		}
		if _, err := tokensOf(before+long, 20, 1000); !errors.Is(err, errXMLTokenTooLong) {
			t.Errorf("%s: a long tag after it is not refused: %v", name, err)
		}
	}
}

// Text is counted as text: a run of it may be much longer than a tag, and is bounded by its own limit, exactly.
func TestTheLengthOfARunOfTextIsBoundedExactly(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		in      string
		refused bool
	}{
		"a run of the limit":             {"<r>" + strings.Repeat("t", 100) + "</r>", false},
		"a run one over":                 {"<r>" + strings.Repeat("t", 101) + "</r>", true},
		"each run is counted on its own": {"<r>" + strings.Repeat(strings.Repeat("t", 100)+"<a/>", 10) + "</r>", false},
		"text before the first tag":      {strings.Repeat("t", 101) + "<r/>", true},
	} {
		_, err := tokensOf(tc.in, 1000, 100)
		if refused := errors.Is(err, errXMLTokenTooLong); refused != tc.refused {
			t.Errorf("%s: refused = %v (%v), want %v", name, refused, err, tc.refused)
		}
	}
	// What is passed on is what came in, up to the refusal.
	var out bytes.Buffer
	if _, err := io.Copy(&out, newXMLMeter(strings.NewReader("<a>text</a>"), 100, 100)); err != nil || out.String() != "<a>text</a>" {
		t.Errorf("passed on %q, %v", out.String(), err)
	}
}

// The start tags of the elements that are open are counted together: tags that are each well within the limit
// could still cost a great deal when many are nested, since the decoder holds the namespaces of every one that is
// open. Tags that are closed again are not counted.
func TestTheStartTagsThatAreOpenAreBoundedTogether(t *testing.T) {
	t.Parallel()

	tag := `<w:sdt` + strings.Repeat(` xmlns:n="u"`, 340) + `>` // about four kilobytes
	nested := func(n int) string {
		return wordOpen + strings.Repeat(tag, n) + strings.Repeat(`</w:sdt>`, n) + wordClose
	}
	if len(tag) < 3<<10 || len(tag) > 5<<10 {
		t.Fatalf("the tag is %d bytes, and the test counts on about four kilobytes", len(tag))
	}
	// Sixty tags of four kilobytes are open together: under the limit.
	if _, err := readDOCX(wordDocument(t, nested(60)), DefaultLimits); err != nil {
		t.Errorf("60 tags of %d bytes, nested: %v", len(tag), err)
	}
	// A hundred and twenty are over it, though each is a small tag.
	if _, err := readDOCX(wordDocument(t, nested(120)), DefaultLimits); !errors.Is(err, ErrMalformed) || !strings.Contains(err.Error(), "too long together") {
		t.Errorf("120 tags of %d bytes, nested: %v, want them refused together", len(tag), err)
	}
	// The same tags one after another are not open together.
	flat := wordOpen + strings.Repeat(tag+`</w:sdt>`, 500) + wordClose
	if _, err := readDOCX(wordDocument(t, flat), DefaultLimits); err != nil {
		t.Errorf("500 tags of %d bytes, one after another: %v", len(tag), err)
	}
}

// The start tags that are open together are bounded exactly: tags that come to the limit are read, and one byte more
// is refused.
func TestTheStartTagsThatAreOpenAreBoundedToTheByte(t *testing.T) {
	t.Parallel()

	root := len(`<w:document xmlns:w="`+nsWordTransitional+`">`) + len(`<w:body>`)
	tag := func(n int) string { return `<w:sdt a="` + strings.Repeat("x", n-12) + `">` } // exactly n bytes
	nested := func(total int) string {
		open := root
		var b strings.Builder
		n := 0
		for open < total {
			size := min(4000, total-open)
			if size < 12 { // what is left is too little for a tag: the last one is larger instead
				size += 12
				if open+size > total {
					t.Fatalf("cannot make %d bytes", total)
				}
			}
			b.WriteString(tag(size))
			open += size
			n++
		}
		return wordOpen + b.String() + strings.Repeat(`</w:sdt>`, n) + wordClose
	}
	read := func(total int) error {
		_, err := readDOCX(wordDocument(t, nested(total)), DefaultLimits)
		return err
	}
	if err := read(maxXMLOpenTags); err != nil {
		t.Errorf("open start tags that come to exactly %d bytes: %v", maxXMLOpenTags, err)
	}
	if err := read(maxXMLOpenTags + 1); !errors.Is(err, ErrMalformed) || !strings.Contains(err.Error(), "too long together") {
		t.Errorf("open start tags that come to one byte more: %v, want them refused", err)
	}
}

// A document with a comment, a CDATA section or an instruction in it, which holds a quote, is read: what refused
// it was a scanner that took the quote for the start of a value.
func TestADocumentWithAQuoteInACommentOrASectionIsRead(t *testing.T) {
	t.Parallel()

	paragraphs := strings.Repeat(`<w:p><w:r><w:t>avsnitt</w:t></w:r></w:p>`, 2000)
	for name, extra := range map[string]string{
		"a comment":       `<!-- et "sitat" -->`,
		"a CDATA section": `<w:p><w:r><w:t><![CDATA[et "sitat"]]></w:t></w:r></w:p>`,
		"an instruction":  `<?note et "sitat"?>`,
		"an apostrophe":   `<!-- it's -->`,
	} {
		c, err := readDOCX(wordDocument(t, wordOpen+extra+paragraphs+wordClose), DefaultLimits)
		if err != nil || len(c.Units) < 2000 {
			t.Errorf("%s: %d units, %v", name, len(c.Units), err)
		}
	}
}

// A run of text is held whole by the decoder, and the character limit cuts what is read only after it: a run
// longer than the limit allows is refused as it is read.
func TestARunOfTextLongerThanTheCharacterLimitAllowsIsRefused(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 1000
	run := func(n int) []byte {
		return wordDocument(t, wordOpen+`<w:p><w:r><w:t>`+strings.Repeat("a", n)+`</w:t></w:r></w:p>`+wordClose)
	}
	if c, err := readDOCX(run(int(xmlTextLimit(lim.MaxChars))-len(`<w:t>`)-2), lim); err != nil || c.Chars != 1000 {
		t.Errorf("a run within its limit: %d characters, %v", c.Chars, err)
	}
	if _, err := readDOCX(run(int(xmlTextLimit(lim.MaxChars))+1), lim); !errors.Is(err, ErrMalformed) || !strings.Contains(err.Error(), "too long") {
		t.Errorf("a run past its limit: %v", err)
	}
	if got, want := xmlTextLimit(1000), int64(8*1000+4096); got != want {
		t.Errorf("the limit of a run of text for a limit of 1000 characters is %d, want %d", got, want)
	}
	if xmlTextLimit(0) != 64<<20 {
		t.Errorf("the limit of a run of text with no limit on characters is %d", xmlTextLimit(0))
	}
}

// The numbers the documentation gives.
func TestTheLimitsOnTheMainPartAreTheOnesTheDocsGive(t *testing.T) {
	t.Parallel()

	if maxXMLTag != 64<<10 || maxXMLOpenTags != 256<<10 || maxXMLDepth != 256 {
		t.Errorf("a token of markup %d, open tags together %d and depth %d, want 64 KiB, 256 KiB and 256", maxXMLTag, maxXMLOpenTags, maxXMLDepth)
	}
}

func TestADOCXWithTooManyPartsIsNotOne(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for i := 0; i <= maxDOCXEntries; i++ {
		w, _ := zw.Create(fmt.Sprintf("p%d", i))
		_, _ = w.Write([]byte("x"))
	}
	w, _ := zw.Create("word/document.xml")
	_, _ = w.Write([]byte(wordOpen + `<w:p><w:r><w:t>x</w:t></w:r></w:p>` + wordClose))
	_ = zw.Close()
	if _, err := readDOCX(buf.Bytes(), DefaultLimits); !errors.Is(err, ErrMalformed) {
		t.Errorf("err = %v, want ErrMalformed", err)
	}
	if got := Detect(buf.Bytes()); got != KindOther {
		t.Errorf("Detect = %q: a ZIP with more parts than any office file has is not a DOCX", got)
	}
}

func TestTheMainPartOfADOCXIsHeldToItsLimitExactly(t *testing.T) {
	t.Parallel()

	xml := wordOpen + `<w:p><w:r><w:t>x</w:t></w:r></w:p>` + wordClose
	data := wordDocument(t, xml)
	lim := DefaultLimits
	lim.MaxXMLBytes = int64(len(xml))
	if _, err := readDOCX(data, lim); err != nil {
		t.Errorf("a part of exactly the limit: %v", err)
	}
	lim.MaxXMLBytes = int64(len(xml)) - 1
	if _, err := readDOCX(data, lim); !errors.Is(err, ErrMalformed) {
		t.Errorf("a part one byte over: err = %v, want ErrMalformed", err)
	}
}

func TestTheMainPartIsFoundWhateverTheCaseOfItsName(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	w, _ := zw.Create("WORD/Document.XML")
	_, _ = w.Write([]byte(wordOpen + `<w:p><w:r><w:t>funnet</w:t></w:r></w:p>` + wordClose))
	_ = zw.Close()
	if c, err := readDOCX(buf.Bytes(), DefaultLimits); err != nil || len(c.Units) != 1 || c.Units[0].Text != "funnet" {
		t.Errorf("%+v, %v", c, err)
	}
}

func TestElementsNestedTooDeeplyAreRefusedAndDepthIsCountedBothWays(t *testing.T) {
	t.Parallel()

	// The document and the body are two levels, and a paragraph, a run and a text are three more.
	nested := func(n int) string {
		return wordOpen + strings.Repeat(`<w:sdt>`, n) + `<w:p><w:r><w:t>dypt</w:t></w:r></w:p>` + strings.Repeat(`</w:sdt>`, n) + wordClose
	}
	deepest := func(n int) int { return n + 5 }
	// Every one of these is well formed: only the depth can refuse it.
	inside := maxXMLDepth - 5
	if deepest(inside) != maxXMLDepth {
		t.Fatalf("the test counts the levels wrongly")
	}
	if c, err := readDOCX(wordDocument(t, nested(inside)), DefaultLimits); err != nil || len(c.Units) != 1 {
		t.Errorf("nesting to exactly %d levels: %v", maxXMLDepth, err)
	}
	if _, err := readDOCX(wordDocument(t, nested(inside+1)), DefaultLimits); !errors.Is(err, ErrMalformed) || !strings.Contains(err.Error(), "nested too deeply") {
		t.Errorf("nesting to %d levels: %v, want it refused for its depth", maxXMLDepth+1, err)
	}
	// Many elements one after another are not depth: the count goes down as they close.
	flat := wordOpen + strings.Repeat(`<w:p><w:r><w:t>a</w:t></w:r></w:p>`, 5*maxXMLDepth) + wordClose
	if c, err := readDOCX(wordDocument(t, flat), DefaultLimits); err != nil || len(c.Units) != 5*maxXMLDepth {
		t.Errorf("a long flat document: %d units, %v", len(c.Units), err)
	}
}

func TestWhatIsDeletedIsNotTextAndDeletionsNest(t *testing.T) {
	t.Parallel()

	xml := wordOpen +
		`<w:p><w:r><w:t>ett</w:t></w:r><w:del><w:r><w:t>borte</w:t></w:r></w:del><w:r><w:t>to</w:t></w:r></w:p>` +
		// a deletion inside a deletion: what follows the inner one is still deleted
		`<w:p><w:del><w:del><w:r><w:t>a</w:t></w:r></w:del><w:r><w:t>b</w:t></w:r></w:del><w:r><w:t>sett</w:t></w:r></w:p>` +
		// a tab or a break inside a deletion is not a space
		`<w:p><w:r><w:t>x</w:t></w:r><w:del><w:r><w:tab/></w:r></w:del><w:r><w:t>y</w:t></w:r></w:p>` +
		`<w:p><w:r><w:t>x</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>y</w:t></w:r></w:p>` +
		wordClose
	c, err := readDOCX(wordDocument(t, xml), DefaultLimits)
	if err != nil {
		t.Fatal(err)
	}
	got := texts(c.Units)
	want := []string{"ettto", "sett", "xy", "x y"}
	if len(got) != len(want) {
		t.Fatalf("units %q, want %q", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("unit %d = %q, want %q", i+1, got[i], want[i])
		}
	}
}

func TestOnlyWordElementsAreTextAndBothWordNamespacesAreWord(t *testing.T) {
	t.Parallel()

	other := `<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:x="urn:other"><w:body>` +
		`<w:p><w:r><w:t>ord</w:t><x:t>annet</x:t></w:r></w:p>` + wordClose
	if c, err := readDOCX(wordDocument(t, `<?xml version="1.0"?>`+other), DefaultLimits); err != nil || len(c.Units) != 1 || c.Units[0].Text != "ord" {
		t.Errorf("another namespace: %+v, %v", c, err)
	}
	strict := `<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p><w:r><w:t>streng</w:t></w:r></w:p></w:body></w:document>`
	if c, err := readDOCX(wordDocument(t, `<?xml version="1.0"?>`+strict), DefaultLimits); err != nil || len(c.Units) != 1 || c.Units[0].Text != "streng" {
		t.Errorf("a strict document: %+v, %v", c, err)
	}
}

// The reading stops when the unit builder says it is full, not when the file ends: what the paragraphs are handed
// to is what decides, and what is not read costs nothing.
func TestTheParagraphsOfAPartAreReadOnlyAsFarAsTheLimitNeeds(t *testing.T) {
	t.Parallel()

	xml := wordOpen + strings.Repeat(`<w:p><w:r><w:t>0123456789</w:t></w:r></w:p>`, 100_000) + wordClose
	src := &countingText{r: strings.NewReader(xml)}
	var got []string
	err := wordParagraphs(src, 100, func(p string) bool {
		got = append(got, p)
		return len(got) < 10 // the builder is full when it holds ten paragraphs of ten characters
	}, func() {})
	if err != nil || len(got) != 10 {
		t.Fatalf("%d paragraphs, %v: the reading goes on until emit says it is full, and then stops", len(got), err)
	}
	if src.taken > 64<<10 {
		t.Errorf("%d bytes of %d were taken for ten paragraphs: the rest of the file was read", src.taken, len(xml))
	}

	// Through the reader, with the character limit: the units hold exactly the limit.
	lim := DefaultLimits
	lim.MaxChars = 100
	c, err := readDOCX(wordDocument(t, xml), lim)
	if err != nil || c.Chars != 100 || len(c.Units) != 10 || !c.Truncated {
		t.Errorf("units %d, characters %d, truncated %v, %v", len(c.Units), c.Chars, c.Truncated, err)
	}
}

// countingText counts what is taken of it.
type countingText struct {
	r     io.Reader
	taken int64
}

func (c *countingText) Read(p []byte) (int, error) {
	n, err := c.r.Read(p)
	c.taken += int64(n)
	return n, err
}

// Tables nest, and what is in a cell of a table that holds a table is still in that cell: the text before and after
// the inner table, and the cells next to it. The inner table's rows are units of their own, as they are made.
func TestATableInACellDoesNotTakeTheOuterCellWithIt(t *testing.T) {
	t.Parallel()

	cell := func(content string) string { return `<w:tc>` + content + `</w:tc>` }
	p := func(text string) string { return `<w:p><w:r><w:t>` + text + `</w:t></w:r></w:p>` }
	inner := `<w:tbl><w:tr>` + cell(p("indre A")) + cell(p("indre B")) + `</w:tr><w:tr>` + cell(p("indre C")) + `</w:tr></w:tbl>`
	outer := `<w:tbl><w:tr>` + cell(p("før")+inner+p("etter")) + cell(p("nabo")) + `</w:tr><w:tr>` + cell(p("neste rad")) + `</w:tr></w:tbl>`
	c, err := readDOCX(wordDocument(t, wordOpen+p("innledning")+outer+p("avslutning")+wordClose), DefaultLimits)
	if err != nil {
		t.Fatal(err)
	}
	if got, want := strings.Join(texts(c.Units), "\n"), strings.Join([]string{
		"innledning",
		"indre A | indre B", // the inner table's rows come as they end, which is before the outer row
		"indre C",
		"før etter | nabo", // the outer cell keeps what was before and after the inner table, and its neighbour is still there
		"neste rad",
		"avslutning",
	}, "\n"); got != want {
		t.Errorf("units:\n%s\nwant:\n%s", got, want)
	}
}

// A row or a cell outside any table, in a document that is not well made, is read as if it had one.
func TestARowOutsideATableIsStillRead(t *testing.T) {
	t.Parallel()

	xml := wordOpen + `<w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr><w:p><w:r><w:t>c</w:t></w:r></w:p>` + wordClose
	c, err := readDOCX(wordDocument(t, xml), DefaultLimits)
	if err != nil || strings.Join(texts(c.Units), "|") != "a | b|c" {
		t.Errorf("units %q, %v", texts(c.Units), err)
	}
}

// Text that is cut by the clip before it is cleaned is reported as cut, whatever the character limit says: a
// paragraph that is mostly blanks loses its tail while it is far under the limit.
func TestAClipIsReportedAsTruncated(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 100
	blanks := strings.Repeat(" ", cleaningLimit(lim.MaxChars)+10) // more than is worth cleaning, which cleans to nothing
	for name, read := range map[string]func() (Content, error){
		"text": func() (Content, error) { return readText([]byte("ord"+blanks+"slutt"), lim) },
		"html": func() (Content, error) { return readHTML([]byte("<p>ord"+blanks+"slutt</p>"), lim) },
		"docx": func() (Content, error) {
			return readDOCX(wordDocument(t, wordOpen+`<w:p><w:r><w:t xml:space="preserve">ord`+blanks+`slutt</w:t></w:r></w:p>`+wordClose), lim)
		},
	} {
		c, err := read()
		if err != nil || !c.Truncated {
			t.Errorf("%s: truncated %v, %v: text was left unread and the file does not say so", name, c.Truncated, err)
		}
	}
	// A paragraph that is not cut is not reported.
	if c, _ := readText([]byte("ord slutt"), lim); c.Truncated {
		t.Errorf("a short text is reported as truncated")
	}
	// Nor is a page of a PDF that is under what is worth cleaning.
	r := NewReader(fakePDF{pages: []string{"en side"}}, lim)
	if c, err := r.readPDF(t.Context(), miniPDF("x")); err != nil || c.Truncated {
		t.Errorf("a short page: truncated %v, %v", c.Truncated, err)
	}
	r = NewReader(fakePDF{pages: []string{"ord" + blanks + "slutt"}}, lim)
	if c, err := r.readPDF(t.Context(), miniPDF("x")); err != nil || !c.Truncated {
		t.Errorf("a page that is cut: truncated %v, %v", c.Truncated, err)
	}
}

// A page of a PDF is clipped before it is cleaned, as a paragraph is: a page of tens of megabytes is not cleaned
// whole for the sake of its first characters. Not parallel: the figure is the process's.
func TestAPageOfAPDFIsNotCleanedWhole(t *testing.T) {
	lim := DefaultLimits
	lim.MaxChars = 30_000
	page := strings.Repeat("ord ", 2<<20) // 8 MiB
	r := NewReader(fakePDF{pages: []string{page}}, lim)
	var c Content
	var err error
	if cost := spent(func() { c, err = r.readPDF(t.Context(), miniPDF("x")) }); err != nil || cost > 6<<20 || c.Chars != lim.MaxChars || !c.Truncated {
		t.Errorf("a page of 8 MiB: %d MiB, %d characters, truncated %v, %v; cleaning it whole costs about 100 MiB", cost>>20, c.Chars, c.Truncated, err)
	}
}

// Text that is not in a paragraph, and is the last in the file, is still text: the reading ends the paragraph it
// was in when the file ends.
func TestTextThatIsNotInAParagraphAtTheEndOfTheFileIsStillRead(t *testing.T) {
	t.Parallel()

	c, err := readDOCX(wordDocument(t, wordOpen+`<w:r><w:t>løs tekst</w:t></w:r>`+wordClose), DefaultLimits)
	if err != nil || strings.Join(texts(c.Units), "|") != "løs tekst" {
		t.Errorf("units %q, %v", texts(c.Units), err)
	}
}

// fakePDF is a PDF reader that answers with the pages it was given.
type fakePDF struct{ pages []string }

func (f fakePDF) Run(_ context.Context, _ []byte, _ int) (PDFResult, error) {
	return PDFResult{Pages: f.pages, TotalPages: len(f.pages)}, nil
}

func TestAnErrorInTheXMLIsAnErrorAndNotWhatWasReadBefore(t *testing.T) {
	t.Parallel()

	if _, err := readDOCX(wordDocument(t, wordOpen+`<w:p><w:r><w:t>x</w:t></w:r></w:p><w:p><`), DefaultLimits); !errors.Is(err, ErrMalformed) {
		t.Errorf("err = %v, want ErrMalformed", err)
	}
}

// A paragraph of many blanks is not rescanned at every paragraph in it.
func TestParagraphsInsideABlankOneAreNotQuadratic(t *testing.T) {
	t.Parallel()

	xml := wordOpen + `<w:p><w:r><w:t>` + strings.Repeat(" ", 4<<20) + `</w:t></w:r>` + strings.Repeat(`<w:p>`, maxXMLDepth-6) + strings.Repeat(`</w:p>`, maxXMLDepth-6) + `</w:p>` + wordClose
	start := time.Now()
	if _, err := readDOCX(wordDocument(t, xml), DefaultLimits); err != nil {
		t.Fatal(err)
	}
	if took := time.Since(start); took > 5*time.Second {
		t.Errorf("took %v", took)
	}
}

// ---------------------------------------------------------------- HTML

func TestBlocksAroundBlanksAreNotParagraphs(t *testing.T) {
	t.Parallel()

	c, err := readHTML([]byte("<html><body>"+strings.Repeat("<p> </p>", 100_000)+"<p>ord</p></body></html>"), DefaultLimits)
	if err != nil || len(c.Units) != 1 || c.Units[0].Text != "ord" {
		t.Errorf("%d units, %v", len(c.Units), err)
	}
}

func TestANestedSkippedElementIsSkippedToItsOwnEnd(t *testing.T) {
	t.Parallel()

	c, err := readHTML([]byte("<html><body><p>før</p><svg><svg>indre</svg>mellom</svg><p>etter</p></body></html>"), DefaultLimits)
	if err != nil {
		t.Fatal(err)
	}
	if got := strings.Join(texts(c.Units), "|"); got != "før|etter" {
		t.Errorf("units %q, want the text of neither svg", got)
	}
}

// A tag larger than the tokenizer's buffer ends the reading, and what came before it is kept.
func TestATagLargerThanTheBufferEndsTheReadingAndKeepsWhatCameBefore(t *testing.T) {
	t.Parallel()

	data := []byte("<html><body><p>før</p><p " + strings.Repeat("a=b ", htmlMaxBuffer/4+10) + `>etter</p></body></html>`)
	c, err := readHTML(data, DefaultLimits)
	if err != nil || len(c.Units) != 1 || c.Units[0].Text != "før" {
		t.Errorf("units %v, %v", texts(c.Units), err)
	}
}

func TestReadingHTMLStopsAtTwiceTheLimit(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 100
	data := []byte("<html><body>" + strings.Repeat("<p>0123456789</p>", 100_000) + "</body></html>")
	c, err := readHTML(data, lim)
	if err != nil || c.Chars != 100 || !c.Truncated {
		t.Errorf("chars %d truncated %v, %v", c.Chars, c.Truncated, err)
	}
}

// The paragraphs of an HTML file are kept until the file is read, so the reading stops when it holds far more
// than the limit, and the cost of a file is not the number of its paragraphs. Not parallel: the figure is
// the process's.
func TestReadingHTMLDoesNotGoOnPastTwiceTheLimit(t *testing.T) {
	lim := DefaultLimits
	lim.MaxChars = 100
	data := []byte("<html><body>" + strings.Repeat("<p>0123456789</p>", 1_000_000) + "</body></html>")
	var c Content
	if cost := spent(func() { c, _ = readHTML(data, lim) }); cost > 4<<20 {
		t.Errorf("reading a file of a million paragraphs with a limit of %d characters allocated %d bytes: it was read to the end", lim.MaxChars, cost)
	}
	if c.Chars != 100 || !c.Truncated {
		t.Errorf("chars %d truncated %v", c.Chars, c.Truncated)
	}
}

// Cleaning a paragraph takes many times its length, and the character limit is applied only after it: one paragraph
// of tens of megabytes cost the better part of a gigabyte to be cut at the limit. What is past four bytes for each
// character the file may yield cannot be kept, and is not cleaned. Not parallel: the figure is the process's.
func TestOneHugeParagraphIsNotCleanedWhole(t *testing.T) {
	lim := DefaultLimits
	lim.MaxChars = 30_000 // so that what is worth cleaning is 124 KB
	rng := rand.New(rand.NewSource(3))
	var block strings.Builder
	for block.Len() < 4096 {
		fmt.Fprintf(&block, "ord%d ", rng.Intn(500))
	}
	prose := strings.Repeat(block.String(), 2048) // 8 MiB: sixty times what is worth cleaning
	var c Content
	var err error

	// A text file of one line. The file itself is copied once to be read as text, which is 8 MiB of the cost.
	text := []byte(prose)
	if cost := spent(func() { c, _ = readText(text, lim) }); cost > 24<<20 || c.Chars != lim.MaxChars {
		t.Errorf("text: %d MiB for a paragraph of 8 MiB, %d characters; cleaning it whole costs about 100 MiB", cost>>20, c.Chars)
	}

	// A DOCX whose one paragraph is forty runs of 200 KB: each run is within its limit, and together they are 8 MB.
	runs := strings.Repeat(`<w:r><w:t>`+prose[:200_000]+`</w:t></w:r>`, 40)
	docx := wordDocument(t, wordOpen+`<w:p>`+runs+`</w:p>`+wordClose)
	if cost := spent(func() { c, err = readDOCX(docx, lim) }); err != nil || cost > 24<<20 || c.Chars != lim.MaxChars {
		t.Errorf("docx: %d MiB for a paragraph of %d MiB, %d characters, %v; cleaning it whole costs about 100 MiB", cost>>20, len(runs)>>20, c.Chars, err)
	}

	// An HTML file with a paragraph of 2 MB.
	html := []byte("<html><body><p>" + prose[:2<<20] + "</p></body></html>")
	if cost := spent(func() { c, _ = readHTML(html, lim) }); cost > 12<<20 || c.Chars != lim.MaxChars {
		t.Errorf("html: %d MiB, %d characters", cost>>20, c.Chars)
	}
}

func TestClipCutsAtWhatIsWorthCleaningAndAtACharacter(t *testing.T) {
	t.Parallel()

	if got := cleaningLimit(10); got != 4*10+4096 {
		t.Errorf("cleaningLimit(10) = %d, want %d", got, 4*10+4096)
	}
	if got := cleaningLimit(0); got <= 1<<40 {
		t.Errorf("cleaningLimit(0) = %d: with no limit on characters, nothing is cut", got)
	}
	limit := cleaningLimit(10)
	for name, tc := range map[string]struct {
		in   string
		want int
	}{
		"at the limit":     {strings.Repeat("a", limit), limit},
		"one over":         {strings.Repeat("a", limit+1), limit},
		"far over":         {strings.Repeat("a", 10*limit), limit},
		"under the limit":  {"kort", 4},
		"empty":            {"", 0},
		"in a character":   {strings.Repeat("å", limit), limit},             // two bytes each, and the limit is even
		"in a long one":    {strings.Repeat("a", limit-1) + "😀", limit - 1}, // the character does not fit, and is left out whole
		"after a long one": {strings.Repeat("a", limit-4) + "😀" + "bb", limit},
	} {
		got, cut := clip(tc.in, 10)
		if len(got) != tc.want || !utf8.ValidString(got) || !strings.HasPrefix(tc.in, got) {
			t.Errorf("%s: clip left %d bytes, valid %v; want %d", name, len(got), utf8.ValidString(got), tc.want)
		}
		if cut != (len(got) < len(tc.in)) {
			t.Errorf("%s: clip says it cut: %v, and it left %d of %d bytes", name, cut, len(got), len(tc.in))
		}
	}
	if got, cut := clip(strings.Repeat("a", 1<<20), 0); len(got) != 1<<20 || cut {
		t.Errorf("clip with no limit on characters cut %d bytes, and says %v", 1<<20-len(got), cut)
	}
}

// A unit costs a good deal more than the character it may hold: a file of millions of one-letter paragraphs would
// cost hundreds of megabytes in units, and the finder a pass for each. A file yields at most MaxUnits of them, and
// is truncated when it has more, exactly.
func TestAFileYieldsNoMoreUnitsThanTheLimitAllows(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxUnits = 100
	paragraphs := func(n int) []string {
		out := make([]string, n)
		for i := range out {
			out[i] = "a"
		}
		return out
	}
	reads := map[string]func(n int) (Content, error){
		"text": func(n int) (Content, error) { return readText([]byte(strings.Join(paragraphs(n), "\n")), lim) },
		"html": func(n int) (Content, error) {
			return readHTML([]byte("<html><body><p>"+strings.Join(paragraphs(n), "</p><p>")+"</p></body></html>"), lim)
		},
		"docx": func(n int) (Content, error) {
			return readDOCX(wordDocument(t, wordOpen+strings.Repeat(`<w:p><w:r><w:t>a</w:t></w:r></w:p>`, n)+wordClose), lim)
		},
		"rows": func(n int) (Content, error) {
			return readDOCX(wordDocument(t, wordOpen+`<w:tbl>`+strings.Repeat(`<w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr>`, n)+`</w:tbl>`+wordClose), lim)
		},
	}
	for name, read := range reads {
		if c, err := read(100); err != nil || len(c.Units) != 100 || c.Truncated {
			t.Errorf("%s: a file of exactly the limit: %d units, truncated %v, %v", name, len(c.Units), c.Truncated, err)
		}
		if c, err := read(101); err != nil || len(c.Units) != 100 || !c.Truncated {
			t.Errorf("%s: a file of one over: %d units, truncated %v, %v", name, len(c.Units), c.Truncated, err)
		}
		if c, err := read(5000); err != nil || len(c.Units) != 100 || !c.Truncated {
			t.Errorf("%s: a file of far over: %d units, truncated %v, %v", name, len(c.Units), c.Truncated, err)
		}
	}
	// A limit of nothing is no limit.
	lim.MaxUnits = 0
	if c, err := readText([]byte(strings.Join(paragraphs(1000), "\n")), lim); err != nil || len(c.Units) != 1000 || c.Truncated {
		t.Errorf("no limit on units: %d units, truncated %v, %v", len(c.Units), c.Truncated, err)
	}
	if DefaultLimits.MaxUnits != 250_000 {
		t.Errorf("the default limit on units is %d, want 250000", DefaultLimits.MaxUnits)
	}
}

// A line of a text file is cut to what is worth cleaning before it is decoded, since decoding does not make it
// shorter: one line of tens of megabytes is not copied whole for the sake of its first characters. The cut is at the
// start of a character, and is reported.
func TestALongLineOfATextFileIsCutBeforeItIsDecoded(t *testing.T) {
	t.Parallel()

	limit := cleaningLimit(100)
	for name, tc := range map[string]struct {
		in      []byte
		wantMax int // the most bytes of text that may be left
	}{
		"ascii":        {[]byte(strings.Repeat("a", 10*limit)), limit},
		"two-byte":     {[]byte(strings.Repeat("å", 10*limit)), limit},
		"windows-1252": {append([]byte("F\xf8r "), bytes.Repeat([]byte("\xe6"), 10*limit)...), 2 * limit}, // each byte is two bytes of text
	} {
		line, cut, ok := textLines(tc.in, 100)()
		if !ok || !cut || len(line) > tc.wantMax || !utf8.ValidString(line) || line == "" {
			t.Errorf("%s: %d bytes left, cut %v, valid %v; want a cut line of at most %d bytes", name, len(line), cut, utf8.ValidString(line), tc.wantMax)
		}
	}
	// A line that is not too long is not cut, and neither is the next.
	next := textLines([]byte("kort\n"+strings.Repeat("b", 10*limit)+"\nsiste"), 100)
	for i, want := range []struct {
		text string
		cut  bool
	}{{"kort", false}, {strings.Repeat("b", limit), true}, {"siste", false}} {
		if line, cut, ok := next(); !ok || line != want.text || cut != want.cut {
			t.Errorf("line %d: %d bytes, cut %v, %v; want %d bytes, cut %v", i+1, len(line), cut, ok, len(want.text), want.cut)
		}
	}
	if _, _, ok := next(); ok {
		t.Errorf("there is a line after the last")
	}
}

// What a cell holds, and what a row holds, is held to what is worth cleaning too, and each element of them counts
// for the bytes it costs besides its text: a cell of many paragraphs is not gathered whole to be cut at the limit
// afterwards, and a cell of millions of one-byte paragraphs is not the same as one of a few. What is left out is
// reported as a cut.
func TestACellAndARowAreNotGatheredPastWhatIsWorthCleaning(t *testing.T) {
	t.Parallel()

	p := func(text string) string { return `<w:p><w:r><w:t>` + text + `</w:t></w:r></w:p>` }
	cell := func(n int, text string) string { return `<w:tc>` + strings.Repeat(p(text), n) + `</w:tc>` }
	cuts := 0
	longest := func(xml string) (most, elements int) {
		cuts = 0
		err := wordParagraphs(strings.NewReader(wordOpen+xml+wordClose), 100, func(row string) bool {
			most = max(most, len(row))
			elements = max(elements, 1+strings.Count(row, "\n")+strings.Count(row, " "))
			return true
		}, func() { cuts++ })
		if err != nil {
			t.Fatal(err)
		}
		return most, elements
	}
	limit := cleaningLimit(100)
	if elementWeight != 32 {
		t.Fatalf("an element of a cell weighs %d, and the test is written for 32", elementWeight)
	}
	// A cell of ten thousand paragraphs of ten bytes, which is 100 KB: each costs 42, so about a hundred are kept.
	got, _ := longest(`<w:tbl><w:tr>` + cell(10_000, "0123456789") + `</w:tr></w:tbl>`)
	if kept := limit/(10+32) + 1; got < 10*kept-kept || got > 11*kept+kept {
		t.Errorf("a cell of 100 KB came to %d bytes in its row, want about %d paragraphs of ten bytes and a blank each", got, kept)
	}
	if cuts == 0 {
		t.Errorf("a cell that was cut is not reported as cut")
	}
	// The same bytes in one-byte paragraphs are many more elements, and so are kept fewer: a cell of 100 KB of them
	// is 100 000 paragraphs, of which about a hundred and ten are kept.
	_, elements := longest(`<w:tbl><w:tr>` + cell(100_000, "x") + `</w:tr></w:tbl>`)
	if want := limit / (1 + 32); elements > want+2 || elements < want-2 {
		t.Errorf("a cell of 100 000 one-byte paragraphs kept %d of them, want about %d", elements, want)
	}
	// A row of ten thousand cells of ten bytes.
	got, _ = longest(`<w:tbl><w:tr>` + strings.Repeat(cell(1, "0123456789"), 10_000) + `</w:tr></w:tbl>`)
	if got < limit/(10+32)*10 || got > limit/(10+32)*14+60 {
		t.Errorf("a row of 100 KB came to %d bytes", got)
	}
	if cuts == 0 {
		t.Errorf("a row that was cut is not reported as cut")
	}
	// A cell and a row that are not cut are not reported.
	longest(`<w:tbl><w:tr>` + cell(3, "kort") + cell(2, "lenger") + `</w:tr></w:tbl>`)
	if cuts != 0 {
		t.Errorf("a small table is reported as cut %d times", cuts)
	}
}

// referenceCleanText is cleanText as it was written first, which splits the text into a slice of lines and joins
// them again. The streaming one must give what it gave, on whatever text.
func referenceCleanText(s string) string {
	s = strings.ReplaceAll(s, "\r\n", "\n")
	s = strings.ReplaceAll(s, "\r", "\n")
	s = norm.NFC.String(s)
	var b strings.Builder
	for _, r := range s {
		switch {
		case r == '\n':
			b.WriteByte('\n')
		case r == '\t' || unicode.Is(unicode.Zs, r):
			b.WriteByte(' ')
		case r == 0x00ad, r >= 0x200b && r <= 0x200d, r == 0xfeff, r == 0x2060,
			r >= 0x202a && r <= 0x202e, r >= 0x2066 && r <= 0x2069:
		case unicode.IsControl(r) || r == utf8.RuneError:
		default:
			b.WriteRune(r)
		}
	}
	lines := strings.Split(b.String(), "\n")
	out := make([]string, 0, len(lines))
	blank := 0
	for _, line := range lines {
		line = strings.Join(strings.Fields(line), " ")
		if line == "" {
			blank++
			if blank > 1 {
				continue
			}
		} else {
			blank = 0
		}
		out = append(out, line)
	}
	return strings.TrimSpace(strings.Join(out, "\n"))
}

func TestCleaningTextByStreamingGivesWhatSplittingIntoLinesGave(t *testing.T) {
	t.Parallel()

	pieces := []string{"ord", "Ord", "å", "ø", "æ", " ", "  ", "\t", "\n", "\n\n", "\r", "\r\n", "\u00a0", "\u200b", "\u00ad", "\u202e",
		"\x00", "\x07", "e\u0301", "\ufffd", "\xff", "1.", "-", " \n ", "\n \n", "😀", "ISO 9001"}
	rng := rand.New(rand.NewSource(5))
	for i := 0; i < 20_000; i++ {
		var b strings.Builder
		for n := rng.Intn(30); n > 0; n-- {
			b.WriteString(pieces[rng.Intn(len(pieces))])
		}
		in := b.String()
		if got, want := cleanText(in), referenceCleanText(in); got != want {
			t.Fatalf("cleanText(%q) = %q, want %q", in, got, want)
		}
	}
	for _, in := range []string{"", "\n", "\n\n\n", "  ", "a", "a\n", "\na", "a\n\n\nb", "a \n \n \n b ", strings.Repeat("\n", 1000) + "x" + strings.Repeat("\r", 1000)} {
		if got, want := cleanText(in), referenceCleanText(in); got != want {
			t.Errorf("cleanText(%q) = %q, want %q", in, got, want)
		}
	}
}

// A text of millions of line breaks is not held as millions of lines: the cost of cleaning it is a small multiple of
// its length. Not parallel: the figure is the process's.
func TestCleaningATextOfManyLineBreaksCostsAMultipleOfItsLength(t *testing.T) {
	text := strings.Repeat("\r", 6<<20) + "x" // six million line breaks
	var got string
	if cost := spent(func() { got = cleanText(text) }); cost > 40<<20 || got != "x" {
		t.Errorf("cleaning 6 MiB of line breaks cost %d MiB, and gave %q: it was held as lines (that costs about 200 MiB)", cost>>20, got)
	}
}

// A page with its pictures inside it holds them in tags, as data: addresses. A tag that is larger than the tokenizer
// takes ends the reading, and the file says that the rest was not read.
func TestAnHTMLTokenLargerThanTheBufferEndsTheReadingAndTheFileSaysSo(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	picture := func(n int) string { return `<img src="data:image/png;base64,` + strings.Repeat("A", n) + `">` }

	// A picture of 5 MB is within what the tokenizer takes: the text after it is read, and nothing is cut.
	c, err := readHTML([]byte("<p>før</p>"+picture(5<<20)+"<p>etter</p>"), lim)
	if err != nil || strings.Join(texts(c.Units), "|") != "før|etter" || c.Truncated {
		t.Errorf("a picture of 5 MB: units %q, truncated %v, %v", texts(c.Units), c.Truncated, err)
	}
	// One that is larger than the buffer is not: what came before it is kept, and the file is truncated.
	c, err = readHTML([]byte("<p>før</p>"+picture(htmlMaxBuffer+1)+"<p>etter</p>"), lim)
	if err != nil || strings.Join(texts(c.Units), "|") != "før" || !c.Truncated {
		t.Errorf("a picture past the buffer: units %q, truncated %v, %v", texts(c.Units), c.Truncated, err)
	}
	// An end of the file is not a cut.
	if c, _ := readHTML([]byte("<p>før</p><p>etter</p>"), lim); c.Truncated {
		t.Errorf("a file that ends is reported as truncated")
	}
}

// A paragraph of an HTML file that is made of many runs of text, each within what the tokenizer takes, is not
// gathered whole: what is past what is worth cleaning is left out, and the file says so. Not parallel.
func TestAnHTMLParagraphOfManyRunsIsNotGatheredWhole(t *testing.T) {
	lim := DefaultLimits
	lim.MaxChars = 1000                                                 // so that what is worth cleaning is about 8 KB
	run := strings.Repeat("ord ", 25_000)                               // 100 KB
	html := []byte("<p>" + strings.Repeat(run+"<b></b>", 200) + "</p>") // 20 MB in one paragraph, in runs the tag between them does not end
	var c Content
	if cost := spent(func() { c, _ = readHTML(html, lim) }); cost > 12<<20 || !c.Truncated || c.Chars != 1000 {
		t.Errorf("a paragraph of 20 MB: %d MiB, truncated %v, %d characters; gathering it whole costs 40 MiB or more", cost>>20, c.Truncated, c.Chars)
	}
}

// A line of Windows-1252 may be any bytes, and is cut at the limit and not at a byte that looks like the middle of a
// character: bytes from 0x80 to 0xBF are characters of their own, not what is left of one.
func TestALongLineOfWindows1252IsCutWhereverItIs(t *testing.T) {
	t.Parallel()

	limit := cleaningLimit(100)
	line, cut, ok := textLines(bytes.Repeat([]byte{0xA9}, 10*limit), 100)()
	if !ok || !cut || len(line) != 2*limit { // each byte is a character of two bytes of text, a copyright sign
		t.Errorf("a line of bytes 0xA9: %d bytes of text, cut %v; want the %d characters of the limit, of two bytes each", len(line), cut, limit)
	}
	// In UTF-8 a cut that falls inside a character is moved back to the start of it, whatever the bytes before.
	for _, prefix := range []string{"", "x", "xy"} {
		line, cut, ok := textLines([]byte(prefix+strings.Repeat("å", 10*limit)), 100)()
		if !ok || !cut || !utf8.ValidString(line) || len(line) > limit || len(line) < limit-1 {
			t.Errorf("a line of å after %q: %d bytes, valid %v, cut %v", prefix, len(line), utf8.ValidString(line), cut)
		}
	}
}

// UTF-16 is read a line at a time like the rest, and a line is cut before it is decoded: the units of a line are
// told from the bytes of one by looking two at a time, and a file of tens of megabytes in one line is not decoded
// whole for the sake of its first characters.
func TestUTF16IsReadALineAtATimeAndCutBeforeItIsDecoded(t *testing.T) {
	t.Parallel()

	utf16 := func(order binary.AppendByteOrder, bom bool, s string) []byte {
		var out []byte
		if bom {
			out = order.AppendUint16(out, 0xFEFF)
		}
		for _, u := range utf16pkg.Encode([]rune(s)) {
			out = order.AppendUint16(out, u)
		}
		return out
	}
	for name, order := range map[string]binary.AppendByteOrder{"little endian": binary.LittleEndian, "big endian": binary.BigEndian} {
		next := textLines(utf16(order, true, "første\r\nandre\n\nsiste"), 100)
		var got []string
		for line, cut, ok := next(); ok; line, cut, ok = next() {
			if cut {
				t.Errorf("%s: a short line is cut", name)
			}
			got = append(got, line)
		}
		if want := []string{"første\r", "andre", "", "siste"}; !slices.Equal(got, want) {
			t.Errorf("%s: lines %q, want %q", name, got, want)
		}
		// A code unit that holds a line feed in one of its two bytes is not a line feed.
		tricky := utf16(order, false, "A\u0a00\u0000B")
		if line, _, ok := textLines(append(utf16(order, true, ""), tricky...), 100)(); !ok || line != "A\u0a00\x00B" {
			t.Errorf("%s: a line with a unit that holds a line feed in one byte: %q", name, line)
		}
		// A long line is cut, and not decoded whole.
		big := utf16(order, true, strings.Repeat("a", 10*cleaningLimit(100)))
		if line, cut, ok := textLines(big, 100)(); !ok || !cut || len(line) != cleaningLimit(100) {
			t.Errorf("%s: a long line: %d bytes, cut %v, %v; want the %d of the limit", name, len(line), cut, ok, cleaningLimit(100))
		}
	}
}

// Through the reader, a file of 64 MiB of UTF-16 in one line costs little more than the file: its first characters
// are decoded and nothing else. Not parallel: the figure is the process's.
func TestAFileOfUTF16InOneLineIsNotDecodedWhole(t *testing.T) {
	u16 := make([]byte, 0, 66<<20)
	u16 = binary.LittleEndian.AppendUint16(u16, 0xFEFF)
	for _, u := range utf16pkg.Encode([]rune(strings.Repeat("ord ", 8<<20))) { // 64 MiB
		u16 = binary.LittleEndian.AppendUint16(u16, u)
	}
	lim := DefaultLimits
	lim.MaxChars = 100_000
	var c Content
	if cost := spent(func() { c, _ = readText(u16, lim) }); cost > 12<<20 || !c.Truncated || c.Chars != lim.MaxChars {
		t.Errorf("a file of 64 MiB of UTF-16 in one line: %d MiB, truncated %v, %d characters; decoding it whole costs about 150 MiB", cost>>20, c.Truncated, c.Chars)
	}
}

// One more paragraph than a file may yield is one too many, but a paragraph that holds nothing is not a unit, and a
// file that ends in blank lines has not been cut.
func TestAFileThatEndsInBlankParagraphsIsNotTruncatedByTheUnitLimit(t *testing.T) {
	t.Parallel()

	b := newBuilder(KindText, "text", 1000)
	b.maxUnits = 3
	for _, p := range []string{"a", "b", "c", "", "  ", "\n"} {
		if !b.add(p) {
			t.Fatalf("%q was refused", p)
		}
	}
	if len(b.c.Units) != 3 || b.c.Truncated {
		t.Errorf("%d units, truncated %v: blank paragraphs after the last are not a cut", len(b.c.Units), b.c.Truncated)
	}
	if b.add("d") || !b.c.Truncated || len(b.c.Units) != 3 {
		t.Errorf("a fourth paragraph with text: units %d, truncated %v", len(b.c.Units), b.c.Truncated)
	}
}

// What is left out because there was more than could be kept is reported even when what is kept comes to less than
// the character limit: the text that was left out was blanks, which clean to nothing, and the file would otherwise
// be the file of no text and not the file that was cut.
func TestWhatIsLeftOutOfAParagraphIsReportedWhenNothingElseWouldSayIt(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 1000 // what is worth cleaning is about 8 KB
	blanks := strings.Repeat(" ", 8<<10)
	check := func(name string, c Content, err error) {
		t.Helper()
		if err != nil || c.Chars != 0 || !c.Truncated {
			t.Errorf("%s: %d characters, truncated %v, %v: want no text, and the file said to be cut", name, c.Chars, c.Truncated, err)
		}
	}
	// An HTML paragraph of many runs of text that a tag between them does not end.
	c, err := readHTML([]byte("<p>"+strings.Repeat(blanks+"<b></b>", 2000)+"</p>"), lim)
	check("html", c, err)
	// A DOCX paragraph of many runs, each within what a run may be.
	c, err = readDOCX(wordDocument(t, wordOpen+`<w:p>`+strings.Repeat(`<w:r><w:t xml:space="preserve">`+blanks+`</w:t></w:r>`, 2000)+`</w:p>`+wordClose), lim)
	check("docx", c, err)
}

// ---------------------------------------------------------------- the child process

func TestAChildThatAllocatesWithoutEndIsEndedByItsWatchdog(t *testing.T) {
	t.Parallel()

	start := time.Now()
	_, err := child("balloon", func(c *ChildPDF) { c.Timeout = 25 * time.Second }).Run(context.Background(), []byte("%PDF-1.4"), 10)
	if !errors.Is(err, ErrFailed) {
		t.Errorf("err = %v, want ErrFailed: the child was ended and read as a crash", err)
	}
	if took := time.Since(start); took > 20*time.Second {
		t.Errorf("the child ran for %v: it was ended by its deadline and not by its watchdog", took)
	}
}

// The wiring of the watchdog, end to end: the service binary, started as the PDF reader, is given a limit by
// the parent and ends itself when a file takes it past that. The parent reads that as a crash and the service
// goes on. A file of a few kilobytes does it: it holds a string of twenty-four mebibytes, and the reader holds
// the string.
func TestAPDFThatTakesTheChildPastItsMemoryLimitEndsTheChildAndNotTheService(t *testing.T) {
	// The file is a PDF the reader reads: a small one of the same kind says what is in it.
	if res, err := ExtractPDF(bombPDF(2), 100); err != nil || len(res.Pages) != 1 || len(res.Pages[0]) < 2<<20 {
		t.Fatalf("a small bomb: %d pages, %v: the test needs a PDF that reads, with its text", len(res.Pages), err)
	}
	bomb := bombPDF(24)
	if len(bomb) > 1<<20 {
		t.Fatalf("the PDF is %d bytes: the test needs a small file", len(bomb))
	}

	start := time.Now()
	_, err := child("main", func(c *ChildPDF) {
		c.Args = []string{ChildArg}
		c.MaxMemory = minChildMemory
		c.Timeout = time.Minute
		c.MaxOutput = 64 << 20
	}).Run(context.Background(), bomb, 100)
	if !errors.Is(err, ErrFailed) || !strings.Contains(err.Error(), "crashed") {
		t.Errorf("err = %v, want ErrFailed from a child that crashed: the child was not ended at its limit", err)
	}
	if took := time.Since(start); took > 30*time.Second {
		t.Errorf("the child took %s to be ended", took)
	}
}

// The soft limit is three quarters of the hard one: the collector is made to work before the watchdog ends the
// process, and not only when it is too late. It is read from a child that has limited itself.
func TestTheChildSetsASoftLimitOfThreeQuartersOfItsHardOne(t *testing.T) {
	t.Parallel()

	cmd := exec.Command(os.Args[0])
	cmd.Env = []string{childModeEnv + "=limits"}
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("the child: %v", err)
	}
	lines := strings.Fields(string(out))
	if want := strconv.FormatInt(64<<20/4*3, 10); len(lines) == 0 || lines[0] != want {
		t.Errorf("the soft limit of a child limited to 64 MiB is %q, want %s (48 MiB)", out, want)
	}
	// Where the kernel can be asked, the child asks to be the one it ends when the host is out of memory.
	if runtime.GOOS == "linux" && (len(lines) != 2 || lines[1] != "1000") {
		t.Errorf("the child's oom_score_adj is %q, want 1000", lines)
	}
}

// A ChildPDF that is built by hand and given no environment runs its child with none: not with the parent's, which
// is what a program started with a nil environment gets. The service binary's database password and the plane's
// key are in the parent's. Not parallel: it sets a variable of the process.
func TestAChildPDFBuiltByHandGetsAnEmptyEnvironmentAndNotTheParents(t *testing.T) {
	t.Setenv("ANBUD_SECRET_PROBE", "hemmelig")

	c := &ChildPDF{Exe: "/bin/sh", Args: []string{"-c", `printf '{"pages":["%s"],"total":1}' "$ANBUD_SECRET_PROBE"`, "--"}, Timeout: 10 * time.Second}
	c.once.Do(c.setup)
	if c.Env == nil || len(c.Env) != 0 {
		t.Fatalf("the environment of a child that was given none is %#v, want an empty one", c.Env)
	}
	res, err := c.Run(context.Background(), miniPDF("x"), 10)
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if len(res.Pages) != 1 || res.Pages[0] != "" {
		t.Errorf("the child saw %q of the parent's environment, want nothing", res.Pages)
	}
}

func TestTheChildTakesItsMemoryLimitFromItsArguments(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		args []string
		want uint64
	}{
		"none given":             {[]string{"--max-pages=5"}, defaultChildMemory},
		"a limit":                {[]string{"--max-memory=268435456"}, 268435456},
		"the least there is":     {[]string{"--max-memory=16777216"}, minChildMemory},
		"under the least":        {[]string{"--max-memory=16777215"}, defaultChildMemory},
		"none":                   {[]string{"--max-memory=0"}, defaultChildMemory},
		"not a number":           {[]string{"--max-memory=lots"}, defaultChildMemory},
		"negative":               {[]string{"--max-memory=-1"}, defaultChildMemory},
		"among others":           {[]string{"--max-pages=5", "--max-memory=100000000", "--max-input=7"}, 100000000},
		"a limit then a bad one": {[]string{"--max-memory=100000000", "--max-memory=x"}, 100000000},
	} {
		if got := memoryArgument(tc.args); got != tc.want {
			t.Errorf("%s: %d, want %d", name, got, tc.want)
		}
	}
	if defaultChildMemory != 1<<30 || minChildMemory != 16<<20 {
		t.Errorf("the default is %d and the least %d, want 1 GiB and 16 MiB", defaultChildMemory, minChildMemory)
	}
}

// The parent hands the limit on, with the others, and a parent that is not told one gives the default.
func TestTheParentPassesTheMemoryLimitToTheChild(t *testing.T) {
	t.Parallel()

	c := &ChildPDF{Exe: "x"}
	c.once.Do(c.setup)
	if c.MaxMemory != defaultChildMemory {
		t.Errorf("the default limit of a parent is %d, want %d", c.MaxMemory, defaultChildMemory)
	}
	c = &ChildPDF{Exe: "x", MaxMemory: 123 << 20}
	c.once.Do(c.setup)
	if c.MaxMemory != 123<<20 {
		t.Errorf("a limit that was set is %d", c.MaxMemory)
	}
}

// The watchdog ends a process that holds more than its limit, and only that: one that holds 40 MiB under a limit of 64
// lives on. A limit that fired at half of what it says would end the readers of ordinary documents.
func TestTheWatchdogLeavesAProcessUnderItsLimitAlone(t *testing.T) {
	t.Parallel()

	cmd := exec.Command(os.Args[0])
	cmd.Env = []string{childModeEnv + "=under"}
	out, err := cmd.Output()
	if err != nil || string(out) != "alive 41943040" {
		t.Errorf("a process that holds 40 MiB under a limit of 64 MiB: %q, %v; want it to live on", out, err)
	}
}

func TestAChildPDFSharedByManyGoroutinesIsSetUpOnce(t *testing.T) {
	t.Parallel()

	c := &ChildPDF{Exe: os.Args[0], Env: []string{childModeEnv + "=real"}, Timeout: 20 * time.Second}
	var wg sync.WaitGroup
	for range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if _, err := c.Run(context.Background(), miniPDF("sammen"), 10); err != nil {
				t.Errorf("Run: %v", err)
			}
		}()
	}
	wg.Wait()
}

func TestTheSlotOfAChildIsGivenBack(t *testing.T) {
	t.Parallel()

	c := child("real", func(c *ChildPDF) { c.MaxConcurrent = 1 })
	for i := range 3 {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		_, err := c.Run(ctx, miniPDF("en side"), 10)
		cancel()
		if err != nil {
			t.Fatalf("run %d: %v: the slot of an earlier run was not given back", i+1, err)
		}
	}
}

func TestTheChildIsStartedInTheTemporaryDirectory(t *testing.T) {
	t.Parallel()

	got, err := child("cwd").Run(context.Background(), []byte("%PDF-1.4"), 10)
	if err != nil || len(got.Pages) != 1 {
		t.Fatalf("Run: %+v, %v", got, err)
	}
	want, _ := filepath.EvalSymlinks(os.TempDir())
	have, _ := filepath.EvalSymlinks(got.Pages[0])
	if want != have {
		t.Errorf("the child ran in %q, want the temporary directory %q: not the service's own directory", have, want)
	}
}

func TestTheChildTakesInputUpToItsLimitAndNoMore(t *testing.T) {
	t.Parallel()

	run := func(input string) string {
		var out bytes.Buffer
		RunPDFChild([]string{"--max-input=10"}, strings.NewReader(input), &out)
		return out.String()
	}
	if got := run(strings.Repeat("x", 10)); !strings.Contains(got, "malformed") {
		t.Errorf("input of exactly the limit: %q, want it read (and found to be no PDF)", got)
	}
	if got := run(strings.Repeat("x", 11)); !strings.Contains(got, "too_large") {
		t.Errorf("input one byte over: %q, want too_large", got)
	}
	// An endless input is not read to its end: one byte past the limit is taken, to see that it is over.
	var out bytes.Buffer
	src := &zeros{}
	RunPDFChild([]string{"--max-input=10"}, io.LimitReader(src, 1<<30), &out)
	if !strings.Contains(out.String(), "too_large") {
		t.Errorf("endless input: %q", out.String())
	}
	if src.taken != 11 {
		t.Errorf("took %d bytes of an endless input with a limit of 10, want 11", src.taken)
	}
}

// zeros is an input of nothing but zero bytes, and counts what is taken of it.
type zeros struct{ taken int64 }

func (z *zeros) Read(p []byte) (int, error) {
	z.taken += int64(len(p))
	return len(p), nil
}

func TestTheChildPassesThePageLimitToTheReader(t *testing.T) {
	t.Parallel()

	var out bytes.Buffer
	RunPDFChild([]string{"--max-pages=2"}, bytes.NewReader(miniPDF("a", "b", "c", "d")), &out)
	if !strings.Contains(out.String(), `"total":4`) || !strings.Contains(out.String(), `"truncated":true`) || strings.Contains(out.String(), `"d"`) {
		t.Errorf("answer %q: want two pages read of four", out.String())
	}
}
