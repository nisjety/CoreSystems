// Package sanitize implements §16.5.4 — wiki content sanitization.
//
// Wiki page bodies arrive as Markdown / Logseq blocks but consumers
// (Velion `/knowledge/wiki`) render to HTML. Trusting the consumer to
// scrub leaves a whole class of XSS bugs in their court. Instead we
// scrub server-side using bluemonday's UGC policy and surface the
// result as `safe_html` alongside the raw `content`.
//
// We never mutate `content` — round-trip equality with the proposer's
// input still holds. `SafeHTMLOK` is `false` if the sanitizer had to
// strip anything significant; clients can fall back to a plain-text
// render in that case.
package sanitize

import (
	"strings"

	"github.com/microcosm-cc/bluemonday"
)

// ugcPolicy is the bluemonday UGC policy with a few markdown-friendly
// additions. Built lazily because policy construction allocates.
var ugcPolicy = func() *bluemonday.Policy {
	p := bluemonday.UGCPolicy()
	// Markdown commonly renders `class="language-*"` on code blocks; allow it.
	p.AllowAttrs("class").Matching(bluemonday.SpaceSeparatedTokens).OnElements("code", "pre", "span", "div")
	// Wiki links pass through as <a href="#path">…</a>.
	p.AllowAttrs("href").OnElements("a")
	p.AllowAttrs("name", "id").Matching(bluemonday.SpaceSeparatedTokens).OnElements("a")
	return p
}()

// Sanitize returns a scrubbed HTML representation of `htmlOrMd` plus a
// flag indicating whether the scrub left the input materially intact.
// We treat ANY tag/attribute strip as "not OK" so downstream UIs know
// to render plain text instead of trusting the HTML.
func Sanitize(htmlOrMd string) (safe string, ok bool) {
	if strings.TrimSpace(htmlOrMd) == "" {
		return "", true
	}
	scrubbed := ugcPolicy.Sanitize(htmlOrMd)
	ok = len(scrubbed) == len(htmlOrMd) && scrubbed == htmlOrMd
	return scrubbed, ok
}
