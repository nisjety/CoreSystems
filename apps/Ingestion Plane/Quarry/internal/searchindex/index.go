package searchindex

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/blevesearch/bleve/v2"
	bleveSearch "github.com/blevesearch/bleve/v2/search"

	"github.com/triodelab/quarry/internal/search"
)

// Index wraps a Bleve full-text index with indexing and query helpers.
// It implements search.SearchClient so it can be used as a local search
// source alongside Brave, GitHub, etc.
type Index struct {
	idx  bleve.Index
	mu   sync.RWMutex
	path string
}

// Option configures Index construction.
type Option func(*indexOptions)

type indexOptions struct {
	path string
}

// WithPath sets the on-disk path for persistent storage.
// When empty, Bleve creates an in-memory index.
func WithPath(p string) Option {
	return func(o *indexOptions) { o.path = p }
}

// Open opens or creates a local search index at the given path.
// If path is empty, an in-memory index is created.
func Open(opts ...Option) (*Index, error) {
	o := &indexOptions{}
	for _, fn := range opts {
		fn(o)
	}

	var idx bleve.Index
	var err error

	if o.path != "" {
		idx, err = bleve.Open(o.path)
		if err == bleve.ErrorIndexPathDoesNotExist {
			idx, err = bleve.New(o.path, buildMapping())
		}
	} else {
		idx, err = bleve.NewMemOnly(buildMapping())
	}
	if err != nil {
		return nil, fmt.Errorf("open search index: %w", err)
	}
	return &Index{idx: idx, path: o.path}, nil
}

// Close releases index resources.
func (ix *Index) Close() error {
	if ix == nil || ix.idx == nil {
		return nil
	}
	return ix.idx.Close()
}

// Put indexes a single document. URL is used as the primary key so
// re-indexing the same URL updates the existing entry.
func (ix *Index) Put(doc Document) error {
	if strings.TrimSpace(doc.URL) == "" {
		return fmt.Errorf("document URL is required")
	}
	if doc.IndexedAt.IsZero() {
		doc.IndexedAt = time.Now().UTC()
	}
	if doc.Snippet == "" && len(doc.Body) > 0 {
		limit := 300
		if len(doc.Body) < limit {
			limit = len(doc.Body)
		}
		doc.Snippet = doc.Body[:limit]
	}
	ix.mu.Lock()
	defer ix.mu.Unlock()
	return ix.idx.Index(doc.URL, doc)
}

// PutBatch indexes multiple documents in a single Bleve batch.
func (ix *Index) PutBatch(docs []Document) error {
	if len(docs) == 0 {
		return nil
	}
	ix.mu.Lock()
	defer ix.mu.Unlock()
	batch := ix.idx.NewBatch()
	for i := range docs {
		d := &docs[i]
		if strings.TrimSpace(d.URL) == "" {
			continue
		}
		if d.IndexedAt.IsZero() {
			d.IndexedAt = time.Now().UTC()
		}
		if d.Snippet == "" && len(d.Body) > 0 {
			limit := 300
			if len(d.Body) < limit {
				limit = len(d.Body)
			}
			d.Snippet = d.Body[:limit]
		}
		if err := batch.Index(d.URL, d); err != nil {
			return fmt.Errorf("batch index %q: %w", d.URL, err)
		}
	}
	return ix.idx.Batch(batch)
}

// Delete removes a document by URL.
func (ix *Index) Delete(url string) error {
	ix.mu.Lock()
	defer ix.mu.Unlock()
	return ix.idx.Delete(url)
}

// Count returns the total number of indexed documents.
func (ix *Index) Count() (uint64, error) {
	ix.mu.RLock()
	defer ix.mu.RUnlock()
	return ix.idx.DocCount()
}

// Query executes a full-text search and returns up to limit results.
func (ix *Index) Query(ctx context.Context, query string, limit int) ([]search.Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if limit <= 0 {
		limit = 20
	}

	q := bleve.NewQueryStringQuery(query)
	req := bleve.NewSearchRequestOptions(q, limit, 0, false)
	req.Fields = []string{"url", "title", "snippet", "source", "body"}
	req.Highlight = bleve.NewHighlightWithStyle("html")

	ix.mu.RLock()
	sr, err := ix.idx.SearchInContext(ctx, req)
	ix.mu.RUnlock()
	if err != nil {
		return nil, fmt.Errorf("search index query: %w", err)
	}

	results := make([]search.Result, 0, len(sr.Hits))
	for _, hit := range sr.Hits {
		snippet := stringField(hit, "snippet")
		if highlighted := firstHighlight(hit.Fragments); highlighted != "" {
			snippet = highlighted
		}
		results = append(results, search.Result{
			Title:   stringField(hit, "title"),
			URL:     stringField(hit, "url"),
			Snippet: snippet,
			Source:  "local",
			Type:    "web",
			Score:   hit.Score,
		})
	}
	return results, nil
}

// --- search.SearchClient implementation ---

func (ix *Index) Search(ctx context.Context, _ search.SearchType, opts search.SearchOptions) ([]search.Result, error) {
	query := opts.Query
	if site := strings.TrimSpace(opts.Site); site != "" {
		query = query + " url:" + site
	}
	return ix.Query(ctx, query, opts.Limit)
}

func (ix *Index) Enabled() bool { return ix != nil && ix.idx != nil }

func (ix *Index) Name() string { return "local" }

// --- helpers ---

func stringField(hit *bleveSearch.DocumentMatch, field string) string {
	if hit == nil || hit.Fields == nil {
		return ""
	}
	v, _ := hit.Fields[field].(string)
	return v
}

func firstHighlight(fragments bleveSearch.FieldFragmentMap) string {
	for _, frags := range fragments {
		for _, f := range frags {
			if strings.TrimSpace(f) != "" {
				return f
			}
		}
	}
	return ""
}
