package searchindex

import "time"

// Document represents a single page indexed in the local search corpus.
type Document struct {
	// URL is the canonical key for deduplication.
	URL string `json:"url"`
	// Title from the page.
	Title string `json:"title"`
	// Body is the concatenated text content (markdown / plain text).
	Body string `json:"body"`
	// Snippet is a short summary (first 300 chars of body if not set).
	Snippet string `json:"snippet,omitempty"`
	// Source describes where the document originated (e.g. "scrape", "crawl").
	Source string `json:"source,omitempty"`
	// IndexedAt records when the document was added/updated.
	IndexedAt time.Time `json:"indexedAt"`
}
