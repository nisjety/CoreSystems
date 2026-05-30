package api

import (
	"testing"
	"time"
)

func TestCompactJSON_Valid(t *testing.T) {
	t.Parallel()

	src := `{"a": 1, "b": [1,2,3]}`
	out, err := CompactJSON(src)
	if err != nil {
		t.Fatalf("CompactJSON returned error: %v", err)
	}
	if out == "" {
		t.Fatalf("CompactJSON returned empty string")
	}
}

func TestCompactJSON_Invalid(t *testing.T) {
	t.Parallel()

	src := `{"a": 1,` // invalid JSON
	if _, err := CompactJSON(src); err == nil {
		t.Fatalf("CompactJSON expected error for invalid JSON, got nil")
	}
}

func TestSchemaCache_SetGet_Close(t *testing.T) {
	t.Parallel()

	c := NewSchemaCache(1 * time.Minute)

	key := "schema1"
	val := `{"x":true}`
	compact, err := CompactJSON(val)
	if err != nil {
		t.Fatalf("CompactJSON error: %v", err)
	}

	c.Set(key, compact)
	got, ok := c.Get(key)
	if !ok {
		t.Fatalf("expected key to be present")
	}
	if got != compact {
		t.Fatalf("got %q want %q", got, compact)
	}

	if err := c.Close(); err != nil {
		t.Fatalf("Close first call returned error: %v", err)
	}
	if err := c.Close(); err != nil {
		t.Fatalf("Close second call returned error: %v", err)
	}
}

func TestSchemaCache_GetExpiresStaleEntry(t *testing.T) {
	t.Parallel()

	c := NewSchemaCache(20 * time.Millisecond)
	t.Cleanup(func() {
		if err := c.Close(); err != nil {
			t.Fatalf("Close returned error: %v", err)
		}
	})

	c.Set("schema", `{"x":true}`)
	time.Sleep(30 * time.Millisecond)

	if _, ok := c.Get("schema"); ok {
		t.Fatal("expected stale schema to be evicted on read")
	}
}
