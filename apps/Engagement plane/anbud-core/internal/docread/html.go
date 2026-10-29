package docread

import (
	"bytes"
	"errors"
	"io"
	"strings"
	"unicode/utf8"

	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
	"golang.org/x/text/encoding/charmap"
	"golang.org/x/text/transform"
)

// htmlMaxBuffer bounds the tokenizer's buffer, so one enormous tag cannot make it grow without limit. A page saved
// with its pictures inside it holds them in tags, as data: addresses, and a token larger than this ends the
// reading, which the file says by being truncated.
const htmlMaxBuffer = 16 << 20

// htmlBlocks are the elements that end a paragraph, at their start and at their end.
var htmlBlocks = map[atom.Atom]bool{
	atom.P: true, atom.Div: true, atom.Br: true, atom.Hr: true, atom.Li: true, atom.Ul: true, atom.Ol: true,
	atom.Table: true, atom.Thead: true, atom.Tbody: true, atom.Tr: true, atom.Blockquote: true, atom.Pre: true,
	atom.Section: true, atom.Article: true, atom.Header: true, atom.Footer: true, atom.Nav: true,
	atom.Aside: true, atom.Main: true, atom.Form: true, atom.Fieldset: true, atom.Address: true,
	atom.H1: true, atom.H2: true, atom.H3: true, atom.H4: true, atom.H5: true, atom.H6: true,
	atom.Dl: true, atom.Dt: true, atom.Dd: true, atom.Title: true,
}

// htmlSkipped are the elements whose content is not the document's: scripts, styles, the head.
var htmlSkipped = map[atom.Atom]bool{
	atom.Script: true, atom.Style: true, atom.Head: true, atom.Noscript: true, atom.Template: true, atom.Svg: true,
}

// htmlSource is what the tokenizer reads: the page as UTF-8. The tokenizer takes bytes as they come, so a page that
// is not valid UTF-8 is taken for Windows-1252, as readText takes a file, since that is what older Norwegian
// software writes æøå in: each of them would otherwise be dropped as an invalid byte, and the word with it. A page
// that is valid UTF-8 is read as it is, whatever it says it is, and what it says is not read. A page that is mostly
// UTF-8 with a stray byte is Windows-1252 all the same, as a text file is. It is decoded as the tokenizer reads, so
// that it is not copied.
func htmlSource(data []byte) io.Reader {
	data = bytes.TrimPrefix(data, []byte{0xEF, 0xBB, 0xBF}) // before it is judged, so that the mark is not read as letters
	if utf8.Valid(data) {
		return bytes.NewReader(data)
	}
	return transform.NewReader(bytes.NewReader(data), charmap.Windows1252.NewDecoder())
}

// readHTML reads an HTML file: a paragraph for each block, the text of a table row on one line.
// Scripts, styles and the head are not text, and entities are decoded. The paragraphs go to the unit builder as they
// are made, and the reading stops when it is full.
func readHTML(data []byte, lim Limits) (Content, error) {
	z := html.NewTokenizer(htmlSource(data))
	z.SetMaxBuf(htmlMaxBuffer)

	b := builderFor(KindHTML, "html", lim)
	full := false
	var buf strings.Builder
	flush := func() {
		// A paragraph of blanks is nothing, and a file of nothing but blocks around blanks must not cost a
		// paragraph for each.
		if strings.TrimSpace(buf.String()) != "" && !b.add(buf.String()) {
			full = true
		}
		buf.Reset()
	}
	skipping := 0 // depth of the skipped element we are inside
	var skipAtom atom.Atom

	for !full {
		tt := z.Next()
		if tt == html.ErrorToken {
			// The end of the file, or a break in it (a tag larger than the buffer, say): a broken file
			// keeps what was read before the break, and says that the rest was not read.
			if errors.Is(z.Err(), html.ErrBufferExceeded) {
				b.cut()
			}
			break
		}
		switch tt {
		case html.TextToken:
			if skipping == 0 {
				if buf.Len() < cleaningLimit(lim.MaxChars) {
					buf.Write(z.Text())
				} else {
					b.cut() // what is past that is never kept, and the paragraph is not made larger for it
				}
			}
		case html.StartTagToken, html.EndTagToken, html.SelfClosingTagToken:
			name, _ := z.TagName()
			a := atom.Lookup(name)
			if skipping > 0 {
				switch {
				case a == skipAtom && tt == html.StartTagToken:
					skipping++
				case a == skipAtom && tt == html.EndTagToken:
					skipping--
				}
				continue
			}
			if htmlSkipped[a] && tt == html.StartTagToken {
				flush()
				skipping, skipAtom = 1, a
				continue
			}
			if htmlBlocks[a] {
				flush()
			} else if (a == atom.Td || a == atom.Th) && tt == html.EndTagToken {
				buf.WriteByte(' ') // the cells of a row are one line
			}
		}
	}
	if !full {
		flush()
	}
	return b.c, nil
}
