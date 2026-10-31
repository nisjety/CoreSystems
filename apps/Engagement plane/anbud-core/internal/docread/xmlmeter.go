package docread

import (
	"errors"
	"io"
)

// errXMLTokenTooLong is what the decoder is answered with when a token of the main part takes more of its
// input than a token may.
var errXMLTokenTooLong = errors.New("docread: a token of the main part is too long")

// xmlMeter hands the bytes of an XML part to the decoder one at a time and counts what each token takes of
// them, so that a token that takes more than it may is cut off as it is read, before the decoder has held it:
// the decoder keeps every attribute and every namespace declaration of the tag it is reading, and a tag of
// millions of them costs far more than the file that holds it.
//
// It does not read the markup. It counts what the decoder itself asks for between one token and the next, so
// that nothing in the markup (a quote in a comment, a ">" in a value, a CDATA section, a declaration) can put it
// out of step with the decoder, which is how a scanner of its own is led astray. All it goes by is what the
// decoder goes by: a token is markup when it begins with "<", and text when it does not; and a token that
// follows a run of text begins with the "<" that ended the run, which the decoder has already been handed (the
// last byte it was handed is then "<": no other token ends with one).
type xmlMeter struct {
	r        io.Reader
	buf      [8 << 10]byte
	pos, end int
	err      error

	taken  int64 // bytes handed to the decoder so far
	mark   int64 // taken when the current token began
	first  int   // the first byte handed out since the token began, or -1
	last   byte  // the last byte handed out
	after  bool  // the token follows a run of text: it begins with the "<" that ended the run
	limit  int64 // the most the current token may take; set when its first byte is handed out
	isText bool  // the current token is a run of text; set when its first byte is handed out

	markup int64 // the most a tag, a comment, an instruction, a declaration or a section may take
	text   int64 // the most a run of text may take
}

func newXMLMeter(r io.Reader, markup, text int64) *xmlMeter {
	return &xmlMeter{r: r, markup: markup, text: text, first: -1}
}

// begin says that a token begins, and has to be called before the decoder is asked for each.
func (m *xmlMeter) begin() {
	m.after = m.last == '<'
	m.mark, m.first = m.taken, -1
	if m.after {
		m.mark-- // the "<" was handed out with the text, and is this token's
	}
}

// used is what the token has taken of the input, counted from its "<".
func (m *xmlMeter) used() int64 { return m.taken - m.mark }

// ReadByte is what the decoder reads by.
func (m *xmlMeter) ReadByte() (byte, error) {
	if m.pos == m.end {
		if m.err != nil {
			return 0, m.err
		}
		n, err := m.r.Read(m.buf[:])
		m.pos, m.end = 0, n
		if err != nil {
			m.err = err // handed out after the bytes that came with it
		}
		if n == 0 {
			if m.err == nil {
				m.err = io.ErrNoProgress
			}
			return 0, m.err
		}
	}
	b := m.buf[m.pos]
	m.pos++
	m.taken++
	m.last = b
	if m.first < 0 {
		m.first = int(b)
		m.isText = !m.after && b != '<'
		m.limit = m.markup
		if m.isText {
			m.limit = m.text
		}
	}
	taken := m.taken - m.mark
	if m.isText && b == '<' {
		taken-- // the "<" that ends a run of text belongs to the token after it
	}
	if taken > m.limit {
		m.err, m.pos = errXMLTokenTooLong, m.end
		return 0, m.err
	}
	return b, nil
}

// Read is here so that the meter is a reader. The decoder does not use it: it reads by ReadByte.
func (m *xmlMeter) Read(p []byte) (int, error) {
	for i := range p {
		b, err := m.ReadByte()
		if err != nil {
			return i, err
		}
		p[i] = b
	}
	return len(p), nil
}
