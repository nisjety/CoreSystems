package searchindex

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/quarry/internal/search"
)

func TestOpenClose_InMemory(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatalf("Open in-memory: %v", err)
	}
	if !ix.Enabled() {
		t.Fatal("expected Enabled() == true")
	}
	if ix.Name() != "local" {
		t.Fatalf("expected Name() == local, got %q", ix.Name())
	}
	if err := ix.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
}

func TestOpenClose_Persistent(t *testing.T) {
	dir := t.TempDir()
	ix, err := Open(WithPath(dir + "/testindex"))
	if err != nil {
		t.Fatalf("Open persistent: %v", err)
	}
	if err := ix.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	// Re-open existing index.
	ix2, err := Open(WithPath(dir + "/testindex"))
	if err != nil {
		t.Fatalf("Re-open persistent: %v", err)
	}
	defer ix2.Close()
}

func TestPut_And_Count(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	doc := Document{
		URL:   "https://example.com/page1",
		Title: "Example Page",
		Body:  "This is the body text for testing",
	}
	if err := ix.Put(doc); err != nil {
		t.Fatalf("Put: %v", err)
	}

	cnt, err := ix.Count()
	if err != nil {
		t.Fatal(err)
	}
	if cnt != 1 {
		t.Fatalf("expected count 1, got %d", cnt)
	}
}

func TestPut_EmptyURL(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	if err := ix.Put(Document{URL: ""}); err == nil {
		t.Fatal("expected error for empty URL")
	}
}

func TestPut_AutoSnippet(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	doc := Document{
		URL:  "https://example.com/auto-snippet",
		Body: "short body",
	}
	if err := ix.Put(doc); err != nil {
		t.Fatal(err)
	}
	// Snippet should be auto-generated; we verify via query.
	results, err := ix.Query(context.Background(), "short body", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(results) == 0 {
		t.Fatal("expected at least 1 result")
	}
}

func TestPutBatch(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	docs := []Document{
		{URL: "https://a.com", Title: "A", Body: "Alpha"},
		{URL: "https://b.com", Title: "B", Body: "Bravo"},
		{URL: "https://c.com", Title: "C", Body: "Charlie"},
	}
	if err := ix.PutBatch(docs); err != nil {
		t.Fatalf("PutBatch: %v", err)
	}
	cnt, err := ix.Count()
	if err != nil {
		t.Fatal(err)
	}
	if cnt != 3 {
		t.Fatalf("expected 3 docs, got %d", cnt)
	}
}

func TestPutBatch_Empty(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	if err := ix.PutBatch(nil); err != nil {
		t.Fatalf("PutBatch(nil) should succeed: %v", err)
	}
}

func TestDelete(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	doc := Document{URL: "https://example.com/del", Title: "Delete me", Body: "delete target"}
	if err := ix.Put(doc); err != nil {
		t.Fatal(err)
	}
	if err := ix.Delete(doc.URL); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	cnt, err := ix.Count()
	if err != nil {
		t.Fatal(err)
	}
	if cnt != 0 {
		t.Fatalf("expected 0 after delete, got %d", cnt)
	}
}

func TestQuery_FullText(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	docs := []Document{
		{URL: "https://example.com/go", Title: "Go Programming", Body: "Go is a statically typed language", Source: "scrape", IndexedAt: time.Now()},
		{URL: "https://example.com/rust", Title: "Rust Programming", Body: "Rust is a systems programming language", Source: "scrape", IndexedAt: time.Now()},
	}
	for _, d := range docs {
		if err := ix.Put(d); err != nil {
			t.Fatal(err)
		}
	}

	results, err := ix.Query(context.Background(), "Go statically typed", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(results) == 0 {
		t.Fatal("expected at least 1 result for 'Go statically typed'")
	}
	if results[0].Source != "local" {
		t.Fatalf("expected source 'local', got %q", results[0].Source)
	}
}

func TestQuery_CancelledContext(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err = ix.Query(ctx, "test", 10)
	if err == nil {
		t.Fatal("expected error for cancelled context")
	}
}

func TestQuery_DefaultLimit(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	// Limit <= 0 should default to 20.
	results, err := ix.Query(context.Background(), "anything", -1)
	if err != nil {
		t.Fatal(err)
	}
	// No docs indexed so 0 results, but the function shouldn't panic.
	if results == nil {
		t.Fatal("expected non-nil results slice")
	}
}

func TestSearch_Interface(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	doc := Document{URL: "https://example.com/iface", Title: "Interface Test", Body: "interface search client test body"}
	if err := ix.Put(doc); err != nil {
		t.Fatal(err)
	}

	// Use the SearchClient interface method.
	results, err := ix.Search(context.Background(), "", search.SearchOptions{
		Query: "interface",
		Limit: 5,
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(results) == 0 {
		t.Fatal("expected results from Search()")
	}
}

func TestNilIndex_Enabled(t *testing.T) {
	var ix *Index
	if ix.Enabled() {
		t.Fatal("nil index should not be enabled")
	}
}

func TestNilIndex_Close(t *testing.T) {
	var ix *Index
	if err := ix.Close(); err != nil {
		t.Fatalf("Close on nil should be nil, got %v", err)
	}
}

func TestPut_Upsert(t *testing.T) {
	ix, err := Open()
	if err != nil {
		t.Fatal(err)
	}
	defer ix.Close()

	doc := Document{URL: "https://example.com/upsert", Title: "V1", Body: "first"}
	if err := ix.Put(doc); err != nil {
		t.Fatal(err)
	}
	doc.Title = "V2"
	doc.Body = "second"
	if err := ix.Put(doc); err != nil {
		t.Fatal(err)
	}
	cnt, err := ix.Count()
	if err != nil {
		t.Fatal(err)
	}
	if cnt != 1 {
		t.Fatalf("upsert should keep count at 1, got %d", cnt)
	}
}
