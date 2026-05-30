package scraper

import (
	"strings"
	"testing"
)

func TestDetectPaginationLinks_RelNext(t *testing.T) {
	html := `
	<html>
		<head>
			<link rel="next" href="/page/2">
		</head>
	</html>
	`
	links := DetectPaginationLinks(html)
	if len(links) == 0 {
		t.Fatalf("expected to find rel=next link, got none")
	}
	found := false
	for _, link := range links {
		if link.Type == "next" && strings.Contains(link.Href, "/page/2") {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("rel=next link not found correctly")
	}
}

func TestDetectPaginationLinks_RelPrev(t *testing.T) {
	html := `<link rel="previous" href="/page/1">`
	links := DetectPaginationLinks(html)
	found := false
	for _, link := range links {
		if link.Type == "prev" {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("rel=previous link not detected")
	}
}

func TestDetectPaginationLinks_TextPatternNext(t *testing.T) {
	html := `<a href="/posts?page=2">Next</a>`
	links := DetectPaginationLinks(html)
	found := false
	for _, link := range links {
		if link.Type == "next" && strings.Contains(link.Href, "page=2") {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("text pattern 'Next' not detected")
	}
}

func TestDetectPaginationLinks_TextPatternPrev(t *testing.T) {
	html := `<a href="/posts?page=1">Previous</a>`
	links := DetectPaginationLinks(html)
	found := false
	for _, link := range links {
		if link.Type == "prev" {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("text pattern 'Previous' not detected")
	}
}

func TestDetectPaginationLinks_NumberedPages(t *testing.T) {
	html := `
	<div class="pagination">
		<a href="/posts?page=1">1</a>
		<strong>2</strong>
		<a href="/posts?page=3">3</a>
	</div>
	`
	links := DetectPaginationLinks(html)
	// Should detect page 3 as next from current page 2.
	found := false
	for _, link := range links {
		if link.Type == "next" && strings.Contains(link.Href, "page=3") {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("numbered pagination (page 3 after current 2) not detected")
	}
}

func TestDetectPaginationLinks_NoPages(t *testing.T) {
	html := `<div>Just some content with no pagination</div>`
	links := DetectPaginationLinks(html)
	if len(links) > 0 {
		t.Errorf("should not detect pagination in plain content, got %d links", len(links))
	}
}

func TestHasNextPage_WithNext(t *testing.T) {
	html := `<a href="/page/2">Next</a>`
	if !HasNextPage(html) {
		t.Errorf("HasNextPage should return true when next link exists")
	}
}

func TestHasNextPage_WithoutNext(t *testing.T) {
	html := `<div>No pagination here</div>`
	if HasNextPage(html) {
		t.Errorf("HasNextPage should return false when no next link exists")
	}
}

func TestGetNextPageURL(t *testing.T) {
	html := `<link rel="next" href="/articles?page=5">`
	url := GetNextPageURL(html)
	if !strings.Contains(url, "page=5") {
		t.Errorf("GetNextPageURL should return the next page URL, got %q", url)
	}
}

func TestGetNextPageURL_Empty(t *testing.T) {
	html := `<div>No pagination</div>`
	url := GetNextPageURL(html)
	if url != "" {
		t.Errorf("GetNextPageURL should return empty string when no next link, got %q", url)
	}
}

func TestDetectPaginationLinks_Deduplication(t *testing.T) {
	html := `
	<link rel="next" href="/page/2">
	<a href="/page/2">Next Page</a>
	`
	links := DetectPaginationLinks(html)
	nextCount := 0
	for _, link := range links {
		if link.Type == "next" && strings.Contains(link.Href, "/page/2") {
			nextCount++
		}
	}
	if nextCount > 1 {
		t.Errorf("expected deduplicated links, got %d 'next' links pointing to /page/2", nextCount)
	}
}

func TestDetectPaginationLinks_RealWorldBlog(t *testing.T) {
	html := `
	<html>
		<body>
			<article>...</article>
			<nav class="pagination">
				<a rel="prev" href="/blog/page/1">← Previous</a>
				<span class="page-numbers">
					<a href="/blog/page/1">1</a>
					<span aria-current="page" class="page-numbers current">2</span>
					<a href="/blog/page/3">3</a>
				</span>
				<a rel="next" href="/blog/page/3">Next →</a>
			</nav>
		</body>
	</html>
	`
	links := DetectPaginationLinks(html)
	if len(links) < 2 {
		t.Fatalf("expected at least 2 pagination links (prev + next), got %d", len(links))
	}

	hasNext := false
	hasPrev := false
	for _, link := range links {
		if link.Type == "next" {
			hasNext = true
		}
		if link.Type == "prev" {
			hasPrev = true
		}
	}

	if !hasNext || !hasPrev {
		t.Errorf("real-world blog pagination: hasNext=%v, hasPrev=%v", hasNext, hasPrev)
	}
}
