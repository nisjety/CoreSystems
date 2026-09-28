// Package htmlstrip renders HTML down to plain text for surfaces that need a
// model-readable or search-indexable string, not markup. The implementation
// was lifted verbatim out of internal/emailsync (where it first shipped for
// mail/Teams body rendering) so internal/actions can reuse it for the
// connector-attach picker's mail.messages.get/gmail.messages.get operations
// without importing emailsync itself — emailsync runs as a background sync
// with its own lifecycle/auth, not a per-request action executor, so sharing
// the package rather than the package boundary is the right seam. Behavior
// is unchanged: emailsync's own stripHTMLTags is now a one-line wrapper
// around StripHTMLTags so its existing tests keep proving the same contract.
package htmlstrip

import (
	"html"
	"strings"
	"unicode"
)

// nonTextElements are dropped wholesale, tag and content together, before the
// generic tag-stripping pass below: their content never rendered as visible
// text in a browser either.
var nonTextElements = []string{"style", "script", "head"}

// StripHTMLTags renders s down to plain text: drop non-text elements
// wholesale, drop the remaining tags, unescape entities, and collapse
// whitespace.
//
// A tag becomes a SPACE rather than nothing: "<p>a</p><p>b</p>" is two words,
// and concatenating them into "ab" is a silent corruption that no later
// consumer can undo. The collapse afterwards keeps that from padding the text.
func StripHTMLTags(s string) string {
	for _, element := range nonTextElements {
		s = dropElement(s, element)
	}
	var b strings.Builder
	inTag := false
	for _, r := range s {
		switch {
		case r == '<':
			inTag = true
			b.WriteRune(' ')
		case r == '>':
			inTag = false
		case !inTag:
			b.WriteRune(r)
		}
	}
	return collapseWhitespace(html.UnescapeString(b.String()))
}

// dropElement removes every "<name ...> ... </name>" span, case-insensitively.
//
// An unclosed opening tag drops the REST of the document on purpose: an
// unterminated <style> means everything after it would have rendered as CSS in
// a browser, so carrying it into body_text would not be recovering content, it
// would be inventing it.
func dropElement(s, name string) string {
	open, closing := "<"+name, "</"+name
	from := 0
	for {
		lower := strings.ToLower(s)
		relative := strings.Index(lower[from:], open)
		if relative < 0 {
			return s
		}
		start := from + relative
		// `<head` is a prefix of `<header`, so a hit is only a real tag when a
		// name boundary follows. Keep scanning past a near-miss rather than
		// giving up: returning here would leave a later, genuine <head> in
		// place purely because a <header> happened to come first.
		if !isTagNameBoundary(lower, start+len(open)) {
			from = start + len(open)
			continue
		}
		end := strings.Index(lower[start:], closing)
		if end < 0 {
			// An unterminated <style> means everything after it would have
			// rendered as CSS in a browser. Dropping the remainder is the
			// honest reading; keeping it would invent message text.
			return s[:start]
		}
		rest := s[start+end:]
		gt := strings.Index(rest, ">")
		if gt < 0 {
			return s[:start]
		}
		s = s[:start] + rest[gt+1:]
		from = start
	}
}

func isTagNameBoundary(s string, index int) bool {
	if index >= len(s) {
		return false
	}
	// unicode.IsSpace rather than a list of rune literals: the whitespace
	// that can follow a tag name is every ASCII space form, and spelling
	// them out invites exactly one of them to be missed.
	c := s[index]
	return c == '>' || c == '/' || unicode.IsSpace(rune(c))
}

func collapseWhitespace(s string) string {
	return strings.TrimSpace(strings.Join(strings.Fields(s), " "))
}
