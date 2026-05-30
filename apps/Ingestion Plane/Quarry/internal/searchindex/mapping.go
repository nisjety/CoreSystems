package searchindex

import (
	"github.com/blevesearch/bleve/v2"
	"github.com/blevesearch/bleve/v2/mapping"
)

// buildMapping creates the Bleve index mapping for Document.
// Title and Body are analyzed with the standard analyzer for full-text search.
// URL is indexed as a keyword for exact lookup / deduplication.
func buildMapping() mapping.IndexMapping {
	docMapping := bleve.NewDocumentMapping()

	// Keyword (exact-match) for URL-based dedup.
	keyword := bleve.NewKeywordFieldMapping()
	docMapping.AddFieldMappingsAt("url", keyword)

	// Full-text.
	text := bleve.NewTextFieldMapping()
	text.Analyzer = "standard"
	docMapping.AddFieldMappingsAt("title", text)
	docMapping.AddFieldMappingsAt("body", text)
	docMapping.AddFieldMappingsAt("snippet", text)

	// Keyword for source.
	docMapping.AddFieldMappingsAt("source", keyword)

	// Date for indexedAt.
	dt := bleve.NewDateTimeFieldMapping()
	docMapping.AddFieldMappingsAt("indexedAt", dt)

	im := bleve.NewIndexMapping()
	im.DefaultMapping = docMapping
	return im
}
