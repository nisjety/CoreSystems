package mailtask

import (
	"html"
	"strings"
	"unicode/utf8"
)

// maxMailBodyChars caps the body text forwarded to the gateway, in characters
// (not bytes), so a forwarded newsletter or a thread of quoted replies cannot
// flood a drafting turn. It matches the cap on text attached in chat.
const maxMailBodyChars = 60000

// bodyTruncationMarker ends a body that was cut at the cap. The marker counts
// toward the cap, so the forwarded body never exceeds maxMailBodyChars.
const bodyTruncationMarker = "\n\n[Message body truncated: only the first 60,000 characters were kept.]"

// mailBodyText is the body forwarded to the gateway: the message's plain text
// part when it has one (verbatim), otherwise its HTML part converted to text,
// in both cases capped at maxMailBodyChars with a truncation marker.
func mailBodyText(text, htmlBody string) string {
	body := text
	if strings.TrimSpace(body) == "" {
		body = htmlToText(htmlBody)
	}
	return capBodyChars(body)
}

// capBodyChars limits body to maxMailBodyChars characters including the
// truncation marker, cutting on a character boundary.
func capBodyChars(body string) string {
	if utf8.RuneCountInString(body) <= maxMailBodyChars {
		return body
	}
	keep := maxMailBodyChars - utf8.RuneCountInString(bodyTruncationMarker)
	cut, runes := 0, 0
	for cut < len(body) && runes < keep {
		_, size := utf8.DecodeRuneInString(body[cut:])
		cut += size
		runes++
	}
	return strings.TrimRight(body[:cut], " \t\r\n") + bodyTruncationMarker
}

// htmlParagraphTags separate the text around them with a blank line when they
// open or close.
var htmlParagraphTags = map[string]bool{
	"blockquote": true, "dl": true, "fieldset": true, "figure": true,
	"h1": true, "h2": true, "h3": true, "h4": true, "h5": true, "h6": true,
	"hr": true, "ol": true, "p": true, "pre": true, "table": true, "ul": true,
}

// htmlLineTags end the current line when they open or close.
var htmlLineTags = map[string]bool{
	"address": true, "article": true, "aside": true, "dd": true, "details": true,
	"div": true, "dt": true, "figcaption": true, "footer": true, "form": true,
	"header": true, "li": true, "main": true, "nav": true, "section": true, "tr": true,
}

// htmlRawTextTags are elements whose content is not message text and is dropped
// together with their tags. An unterminated one swallows the rest of the
// document, as a browser would.
var htmlRawTextTags = map[string]bool{
	"script": true, "style": true, "title": true, "template": true,
	"svg": true, "noscript": true, "iframe": true, "object": true,
}

// htmlToText converts an HTML message body to plain text: comments, scripts,
// styles and every tag are removed (a tag's attributes -- hrefs, titles, inline
// styles -- go with it), block elements become line breaks, list items become
// "- " lines, and character references are decoded. Source whitespace is
// layout, not content, so it collapses; only tags create line breaks.
//
// It is a text extractor for a model prompt, not a sanitiser for HTML output:
// the result is plain text and must never be rendered as HTML.
func htmlToText(src string) string {
	var out textWriter
	out.Grow(len(src) / 2)
	for i := 0; i < len(src); {
		c := src[i]
		if c != '<' {
			// Raw newlines and tabs inside a text node are spaces in HTML.
			if c == '\n' || c == '\r' || c == '\t' {
				c = ' '
			}
			out.WriteByte(c)
			i++
			continue
		}
		if strings.HasPrefix(src[i:], "<!--") {
			end := strings.Index(src[i+len("<!--"):], "-->")
			if end < 0 {
				break // unterminated comment: the rest of the document is comment
			}
			i += len("<!--") + end + len("-->")
			continue
		}
		// '<' only opens a tag when a tag name, '/', '!' or '?' follows.
		if i+1 >= len(src) || !isTagStart(src[i+1]) {
			out.WriteByte('<')
			i++
			continue
		}
		end := htmlTagEnd(src, i+1)
		if end < 0 {
			break // unterminated tag: drop the rest rather than leak half a tag
		}
		name, closing := htmlTagName(src[i+1 : end])
		i = end + 1

		if htmlRawTextTags[name] && !closing {
			next := indexASCIIFold(src, "</"+name, i)
			if next < 0 {
				break // unterminated script/style: the rest is its content
			}
			closeEnd := htmlTagEnd(src, next+2)
			if closeEnd < 0 {
				break
			}
			i = closeEnd + 1
			continue
		}
		switch {
		case name == "br":
			out.forceNewline()
		case htmlParagraphTags[name]:
			out.breakTo(2)
		case name == "li" && !closing:
			out.breakTo(1)
			out.WriteString("- ")
		case htmlLineTags[name]:
			out.breakTo(1)
		case name == "td" || name == "th":
			out.WriteByte(' ')
		}
	}
	return normalizeBodyText(html.UnescapeString(out.String()))
}

// textWriter builds extracted text and knows how many line breaks end it, so a
// block tag adds a break only when one is not already there: pretty-printed HTML
// puts whitespace between every pair of tags, and that must not become blank
// lines. Spaces written after a break do not count as content.
type textWriter struct {
	strings.Builder
	trailing int // newlines since the last non-space byte
}

func (w *textWriter) WriteByte(c byte) error {
	if c != ' ' {
		w.trailing = 0
	}
	return w.Builder.WriteByte(c)
}

func (w *textWriter) WriteString(s string) (int, error) {
	if s != "" {
		w.trailing = 0
	}
	return w.Builder.WriteString(s)
}

// forceNewline always ends the line, as an explicit <br> does.
func (w *textWriter) forceNewline() {
	_ = w.Builder.WriteByte('\n')
	w.trailing++
}

// breakTo ensures the text ends with at least n line breaks (1 ends the line, 2
// leaves a blank line) without stacking more on top of existing ones. Nothing is
// written before any text exists.
func (w *textWriter) breakTo(n int) {
	if w.Len() == 0 {
		return
	}
	for w.trailing < n {
		w.forceNewline()
	}
}

func isTagStart(c byte) bool {
	return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c == '/' || c == '!' || c == '?'
}

// htmlTagEnd returns the index of the '>' that closes the tag whose content
// starts at from, or -1. A quote only opens an attribute value directly after
// '=', so a stray apostrophe in unquoted text inside a tag cannot swallow the
// document; a '>' inside a quoted value does not end the tag.
func htmlTagEnd(src string, from int) int {
	var quote, prev byte
	for j := from; j < len(src); j++ {
		ch := src[j]
		if quote != 0 {
			if ch == quote {
				quote, prev = 0, ch
			}
			continue
		}
		switch ch {
		case '>':
			return j
		case '"', '\'':
			if prev == '=' {
				quote = ch
			}
		}
		if ch != ' ' && ch != '\t' && ch != '\n' && ch != '\r' {
			prev = ch
		}
	}
	return -1
}

// htmlTagName returns the lower-cased element name of a tag body (the text
// between '<' and '>') and whether it is a closing tag. Comments, doctypes and
// processing instructions have no name.
func htmlTagName(tag string) (name string, closing bool) {
	if strings.HasPrefix(tag, "/") {
		closing = true
		tag = tag[1:]
	}
	end := 0
	for end < len(tag) {
		c := tag[end]
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9') {
			break
		}
		end++
	}
	return strings.ToLower(tag[:end]), closing
}

// indexASCIIFold finds needle (lower-case ASCII) in s at or after from, ignoring
// ASCII case. Byte offsets stay valid for any UTF-8 input, unlike lower-casing s
// first, which can change its length.
func indexASCIIFold(s, needle string, from int) int {
	for i := from; i+len(needle) <= len(s); i++ {
		if strings.EqualFold(s[i:i+len(needle)], needle) {
			return i
		}
	}
	return -1
}

// normalizeBodyText tidies extracted text: invisible layout characters that
// marketing mail pads its preview text with are removed, runs of spaces and
// tabs collapse, lines are trimmed, and blank lines collapse to one.
func normalizeBodyText(text string) string {
	text = strings.Map(func(r rune) rune {
		switch {
		case r == '\u00a0':
			return ' '
		case r == '\u00ad', r == '\u034f', r >= '\u200b' && r <= '\u200f', r == '\u2060', r == '\ufeff':
			return -1
		case r == '\n', r == '\t', r == ' ':
			return r
		case r < ' ', r == 0x7f:
			return -1
		}
		return r
	}, text)

	var out strings.Builder
	out.Grow(len(text))
	blank := true // suppress leading blank lines
	pendingBlank := false
	for _, line := range strings.Split(text, "\n") {
		line = strings.Join(strings.Fields(line), " ")
		if line == "" {
			if !blank {
				pendingBlank = true
			}
			continue
		}
		if !blank {
			out.WriteByte('\n')
			if pendingBlank {
				out.WriteByte('\n')
			}
		}
		out.WriteString(line)
		blank, pendingBlank = false, false
	}
	return out.String()
}
