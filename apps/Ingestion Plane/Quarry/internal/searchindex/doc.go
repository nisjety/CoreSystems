// Package searchindex provides a Bleve-backed local full-text search index
// that implements the search.SearchClient interface. Scraped pages are indexed
// automatically so Quarry can answer queries from its own corpus without
// depending on external APIs (Brave, etc.).
package searchindex
