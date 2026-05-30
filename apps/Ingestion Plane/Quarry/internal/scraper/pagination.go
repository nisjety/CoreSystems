package scraper

import (
	"regexp"
)

// PaginationLink represents a detected pagination link.
type PaginationLink struct {
	Text string // Visible text (e.g., "Next", "2", "→")
	Href string // URL or relative path
	Type string // "next", "prev", "page", or "unknown"
}

// DetectPaginationLinks scans HTML for common pagination patterns and returns
// detected next/prev links. Patterns include:
// - Explicit "Next" / "Previous" links
// - Numbered page links (current page detection)
// - Arrow symbols (→, >, «, »)
// - Common rel attributes (rel="next", rel="last")
func DetectPaginationLinks(html string) []PaginationLink {
	var links []PaginationLink

	// Strategy 1: Look for rel="next" / rel="prev" in <a> or <link> tags (most reliable).
	links = append(links, findRelNextPrev(html)...)

	// Strategy 2: Look for common text patterns in links.
	links = append(links, findTextPatternLinks(html)...)

	// Strategy 3: Look for numbered pagination.
	links = append(links, findNumberedPagination(html)...)

	return deduplicateLinks(links)
}

// findRelNextPrev searches for link rel="next" and rel="prev" attributes.
func findRelNextPrev(html string) []PaginationLink {
	var links []PaginationLink

	// Match: <link rel="next" href="..."> or <a rel="next" href="...">
	reNext := regexp.MustCompile(`(?i)<(?:link|a)\s+[^>]*rel=["']?next["']?[^>]*href=["']([^"'>\s]+)["']`)
	rePrev := regexp.MustCompile(`(?i)<(?:link|a)\s+[^>]*rel=["']?prev(?:ious)?["']?[^>]*href=["']([^"'>\s]+)["']`)

	if matches := reNext.FindAllStringSubmatch(html, -1); len(matches) > 0 {
		links = append(links, PaginationLink{
			Text: "Next",
			Href: matches[0][1],
			Type: "next",
		})
	}

	if matches := rePrev.FindAllStringSubmatch(html, -1); len(matches) > 0 {
		links = append(links, PaginationLink{
			Text: "Previous",
			Href: matches[0][1],
			Type: "prev",
		})
	}

	return links
}

// findTextPatternLinks searches for links with common pagination text patterns.
func findTextPatternLinks(html string) []PaginationLink {
	var links []PaginationLink

	// Simple patterns for "Next" and "Previous" text.
	reNext := regexp.MustCompile(`(?i)href=["']([^"'>\s]+)["'][^>]*>\s*next\s*<`)
	rePrev := regexp.MustCompile(`(?i)href=["']([^"'>\s]+)["'][^>]*>\s*(?:previous|prev)\s*<`)

	if matches := reNext.FindAllStringSubmatch(html, -1); len(matches) > 0 {
		href := matches[0][1]
		if !containsLink(links, href, "next") {
			links = append(links, PaginationLink{Text: "Next", Href: href, Type: "next"})
		}
	}

	if matches := rePrev.FindAllStringSubmatch(html, -1); len(matches) > 0 {
		href := matches[0][1]
		if !containsLink(links, href, "prev") {
			links = append(links, PaginationLink{Text: "Previous", Href: href, Type: "prev"})
		}
	}

	return links
}

// containsLink checks if a link already exists in the list with the same href and type.
func containsLink(links []PaginationLink, href, typ string) bool {
	for _, link := range links {
		if link.Href == href && link.Type == typ {
			return true
		}
	}
	return false
}

// findNumberedPagination searches for numbered page links (e.g., 1, 2, 3, ..., current is highlighted).
func findNumberedPagination(html string) []PaginationLink {
	var links []PaginationLink

	// Look for patterns like:
	// <span class="current">2</span> or <strong>2</strong> (current page indicator)
	// <a href="?page=3">3</a> (next page link)
	reCurrentPage := regexp.MustCompile(`(?i)<(?:span|strong|b)[^>]*>[\s]*(\d+)[\s]*</(?:span|strong|b)>`)
	currentPageMatch := reCurrentPage.FindStringSubmatch(html)

	if len(currentPageMatch) > 0 {
		currentNum := currentPageMatch[1]

		// Find "next" page link (number one higher than current).
		rePageLink := regexp.MustCompile(`(?i)href=["']([^"'>\s]*[?&]page=(\d+)[^"'>\s]*)["']`)
		matches := rePageLink.FindAllStringSubmatch(html, -1)

		for _, m := range matches {
			pageNum := m[2]
			// Heuristic: if this page number is higher than current, it's likely "next".
			if pageNum > currentNum {
				isDup := false
				for _, existing := range links {
					if existing.Href == m[1] && existing.Type == "next" {
						isDup = true
						break
					}
				}
				if !isDup {
					links = append(links, PaginationLink{
						Text: "Page " + pageNum,
						Href: m[1],
						Type: "next",
					})
					break // Only take the immediate next page.
				}
			}
		}
	}

	return links
}

// deduplicateLinks removes duplicate links (same href and type).
func deduplicateLinks(links []PaginationLink) []PaginationLink {
	seen := make(map[string]bool)
	var unique []PaginationLink
	for _, link := range links {
		key := link.Href + "|" + link.Type
		if !seen[key] {
			seen[key] = true
			unique = append(unique, link)
		}
	}
	return unique
}

// HasNextPage returns true if the HTML likely contains a next page link.
func HasNextPage(html string) bool {
	links := DetectPaginationLinks(html)
	for _, link := range links {
		if link.Type == "next" {
			return true
		}
	}
	return false
}

// GetNextPageURL returns the next page URL if detected, or empty string.
func GetNextPageURL(html string) string {
	links := DetectPaginationLinks(html)
	for _, link := range links {
		if link.Type == "next" {
			return link.Href
		}
	}
	return ""
}
