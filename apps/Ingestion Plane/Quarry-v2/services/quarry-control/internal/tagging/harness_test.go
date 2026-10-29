package tagging

import (
	"context"
	"encoding/json"
	"flag"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

var updateGolden = flag.Bool("update", false, "update golden fixtures")

type goldenCase struct {
	Name     string `json:"name"`
	Seed     uint64 `json:"seed"`
	MaxTags  int    `json:"max_tags,omitempty"`
	Input    Input  `json:"input"`
	Expected []Tag  `json:"expected"`
}

func TestFixtureTagger_Golden(t *testing.T) {
	t.Parallel()

	entries, err := os.ReadDir("testdata")
	if err != nil {
		t.Fatalf("read testdata: %v", err)
	}

	for _, entry := range entries {
		if entry.IsDir() || filepath.Ext(entry.Name()) != ".json" {
			continue
		}
		name := entry.Name()
		t.Run(name, func(t *testing.T) {
			path := filepath.Join("testdata", name)
			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("read %s: %v", path, err)
			}
			var tc goldenCase
			if err := json.Unmarshal(raw, &tc); err != nil {
				t.Fatalf("decode %s: %v", path, err)
			}

			tagger := &FixtureTagger{Seed: tc.Seed, MaxTags: tc.MaxTags}
			got, err := tagger.Generate(context.Background(), tc.Input)
			if err != nil {
				t.Fatalf("generate: %v", err)
			}

			if *updateGolden {
				tc.Expected = got
				out, err := json.MarshalIndent(tc, "", "  ")
				if err != nil {
					t.Fatalf("marshal golden: %v", err)
				}
				if err := os.WriteFile(path, append(out, '\n'), 0o644); err != nil {
					t.Fatalf("write golden: %v", err)
				}
				return
			}

			if !reflect.DeepEqual(got, tc.Expected) {
				t.Fatalf("tag mismatch for %s\nwant: %+v\n got: %+v", name, tc.Expected, got)
			}

			// Determinism: a second call with the same input must match byte-for-byte.
			again, err := tagger.Generate(context.Background(), tc.Input)
			if err != nil {
				t.Fatalf("second generate: %v", err)
			}
			if !reflect.DeepEqual(got, again) {
				t.Fatalf("non-deterministic output for %s", name)
			}
		})
	}
}

func TestFixtureTagger_WhitespaceInsensitive(t *testing.T) {
	t.Parallel()

	tagger := NewFixtureTagger(42)
	a, err := tagger.Generate(context.Background(), Input{
		URL:   "https://example.com/a",
		Title: "Hello   World",
		Body:  "one two\tthree\n\nfour",
	})
	if err != nil {
		t.Fatalf("generate a: %v", err)
	}
	b, err := tagger.Generate(context.Background(), Input{
		URL:   "https://example.com/a",
		Title: "Hello World",
		Body:  "one two three four",
	})
	if err != nil {
		t.Fatalf("generate b: %v", err)
	}
	if !reflect.DeepEqual(a, b) {
		t.Fatalf("whitespace normalization broke determinism\n a: %+v\n b: %+v", a, b)
	}
}

func TestFixtureTagger_SeedSensitivity(t *testing.T) {
	t.Parallel()

	in := Input{URL: "https://example.com/x", Title: "T", Body: "b"}
	a, err := NewFixtureTagger(1).Generate(context.Background(), in)
	if err != nil {
		t.Fatalf("seed 1: %v", err)
	}
	b, err := NewFixtureTagger(2).Generate(context.Background(), in)
	if err != nil {
		t.Fatalf("seed 2: %v", err)
	}
	if reflect.DeepEqual(a, b) {
		t.Fatalf("expected different output for different seeds; got %+v", a)
	}
}

func TestFixtureTagger_ContextCancelled(t *testing.T) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := NewFixtureTagger(0).Generate(ctx, Input{}); err == nil {
		t.Fatalf("expected error from cancelled context")
	}
}
