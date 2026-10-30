package docread

import (
	"bytes"
	"strings"
	"unicode/utf8"

	"golang.org/x/text/encoding/charmap"
	"golang.org/x/text/encoding/unicode"
)

// readText reads a text file, a paragraph for each line. The lines are decoded and taken one at a time and the
// reading stops at the character limit, so that the cost is the limit and not the size of the file.
func readText(data []byte, lim Limits) (Content, error) {
	b := builderFor(KindText, "text", lim)
	next := textLines(data, lim.MaxChars)
	for line, cut, ok := next(); ok; line, cut, ok = next() {
		if cut {
			b.cut()
		}
		if !b.add(line) {
			break
		}
	}
	return b.c, nil
}

// textLines returns the lines of the bytes as UTF-8, one at a time, and whether a line was cut. UTF-16 is
// recognised by its byte order mark. What is not valid UTF-8 is taken for Windows-1252, in which older Norwegian
// software writes æøå. A line is decoded when it is taken, and is cut to what is worth cleaning (see clip) before it
// is decoded, since decoding does not make it shorter: so that a file of tens of megabytes, or one line of them, is
// not copied whole for the sake of its first characters.
func textLines(data []byte, max int) func() (string, bool, bool) {
	limit := cleaningLimit(max)
	switch {
	case bytes.HasPrefix(data, []byte{0xFF, 0xFE}):
		return utf16Lines(data[2:], unicode.LittleEndian, limit)
	case bytes.HasPrefix(data, []byte{0xFE, 0xFF}):
		return utf16Lines(data[2:], unicode.BigEndian, limit)
	}
	data = bytes.TrimPrefix(data, []byte{0xEF, 0xBB, 0xBF})
	decode := func(line []byte) string { return string(line) }
	inCharacters := true // the bytes are UTF-8, in which a cut must not fall inside a character
	if !utf8.Valid(data) {
		inCharacters = false // a byte is a character, whatever its value
		windows := charmap.Windows1252.NewDecoder()
		decode = func(line []byte) string {
			if s, err := windows.Bytes(line); err == nil {
				return string(s)
			}
			return strings.ToValidUTF8(string(line), "")
		}
	}
	return func() (string, bool, bool) {
		if len(data) == 0 {
			return "", false, false
		}
		line, rest, _ := bytes.Cut(data, []byte{'\n'})
		data = rest
		cut := false
		if len(line) > limit {
			n := limit
			for inCharacters && n > 0 && !utf8.RuneStart(line[n]) {
				n--
			}
			line, cut = line[:n], true
		}
		return decode(line), cut, true
	}
}

// utf16Lines takes the lines of UTF-16 text one at a time. A line ends at the code unit for a line feed, which is
// looked for two bytes at a time: a byte of the other half of a unit is not one. A line is cut to the bytes that
// come to the limit once they are decoded, which is at least half of them.
func utf16Lines(data []byte, order unicode.Endianness, limit int) func() (string, bool, bool) {
	feed := [2]byte{0x0A, 0x00}
	if order == unicode.BigEndian {
		feed = [2]byte{0x00, 0x0A}
	}
	decoder := unicode.UTF16(order, unicode.IgnoreBOM).NewDecoder()
	return func() (string, bool, bool) {
		if len(data) == 0 {
			return "", false, false
		}
		end, next := len(data), len(data)
		for i := 0; i+1 < len(data); i += 2 {
			if data[i] == feed[0] && data[i+1] == feed[1] {
				end, next = i, i+2
				break
			}
		}
		line := data[:end]
		data = data[next:]
		cut := false
		if len(line) > 2*limit {
			line, cut = line[:2*limit], true
		}
		if s, err := decoder.Bytes(line); err == nil {
			return string(s), cut, true
		}
		return strings.ToValidUTF8(string(line), ""), cut, true
	}
}
