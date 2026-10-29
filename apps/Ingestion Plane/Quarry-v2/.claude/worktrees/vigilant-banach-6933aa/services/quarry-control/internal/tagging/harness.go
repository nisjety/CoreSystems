// Package tagging provides a deterministic tag generator harness used to
// produce stable fixtures for tests, replays, and golden snapshots.
//
// The harness intentionally avoids external models: callers inject a seed and
// the FixtureTagger derives reproducible tags from the canonical hash of the
// input. This keeps CI hermetic while still exercising downstream pipelines
// that consume Tag slices.
package tagging

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"fmt"
	"math/rand"
	"sort"
	"strings"
)

// Tag is a single classifier output. Score is bounded to [0, 1].
type Tag struct {
	Key   string  `json:"key"`
	Value string  `json:"value"`
	Score float64 `json:"score"`
}

// Input is the canonical document representation fed to a Tagger.
type Input struct {
	URL   string `json:"url"`
	Title string `json:"title"`
	Body  string `json:"body"`
}

// Tagger generates tags for a document. Implementations must be safe for
// concurrent use.
type Tagger interface {
	Generate(ctx context.Context, in Input) ([]Tag, error)
}

// FixtureTagger emits a deterministic Tag set seeded by the input hash plus
// an external seed. The output is stable across runs and platforms.
type FixtureTagger struct {
	// Seed mixes into the per-input hash so callers can request different
	// reproducible variants from the same input.
	Seed uint64
	// MaxTags caps the output. Zero means default (5).
	MaxTags int
}

// NewFixtureTagger returns a FixtureTagger with the given seed.
func NewFixtureTagger(seed uint64) *FixtureTagger {
	return &FixtureTagger{Seed: seed}
}

const defaultMaxTags = 5

// vocabulary is intentionally small and stable. Adding entries is a breaking
// change to golden fixtures.
var vocabulary = []struct {
	Key    string
	Values []string
}{
	{"topic", []string{"engineering", "research", "policy", "product", "operations"}},
	{"language", []string{"en", "no", "de", "es", "fr"}},
	{"sentiment", []string{"positive", "neutral", "negative"}},
	{"length", []string{"short", "medium", "long"}},
	{"audience", []string{"public", "internal", "developer", "executive"}},
}

// Generate produces a deterministic set of tags for the given input.
func (f *FixtureTagger) Generate(ctx context.Context, in Input) ([]Tag, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	max := f.MaxTags
	if max <= 0 {
		max = defaultMaxTags
	}
	if max > len(vocabulary) {
		max = len(vocabulary)
	}

	source := rand.NewSource(int64(mixSeed(f.Seed, canonicalHash(in))))
	rng := rand.New(source)

	// Pick `max` distinct vocabulary entries in deterministic order.
	indices := rng.Perm(len(vocabulary))[:max]
	sort.Ints(indices)

	out := make([]Tag, 0, max)
	for _, idx := range indices {
		entry := vocabulary[idx]
		valueIdx := rng.Intn(len(entry.Values))
		// Score quantized to two decimals for stable JSON encoding.
		score := float64(rng.Intn(101)) / 100.0
		out = append(out, Tag{
			Key:   entry.Key,
			Value: entry.Values[valueIdx],
			Score: score,
		})
	}

	sort.SliceStable(out, func(i, j int) bool {
		if out[i].Key != out[j].Key {
			return out[i].Key < out[j].Key
		}
		return out[i].Value < out[j].Value
	})
	return out, nil
}

// canonicalHash returns a stable 64-bit digest derived from a canonical
// rendering of the input. Whitespace in body/title is normalized so that
// trivially equivalent inputs produce identical hashes.
func canonicalHash(in Input) uint64 {
	h := sha256.New()
	fmt.Fprintf(h, "url=%s\n", strings.TrimSpace(in.URL))
	fmt.Fprintf(h, "title=%s\n", normalizeWS(in.Title))
	fmt.Fprintf(h, "body=%s\n", normalizeWS(in.Body))
	sum := h.Sum(nil)
	return binary.BigEndian.Uint64(sum[:8])
}

func normalizeWS(s string) string {
	return strings.Join(strings.Fields(s), " ")
}

func mixSeed(seed, h uint64) uint64 {
	// SplitMix64-style mix for good seed dispersion.
	x := seed ^ h
	x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9
	x = (x ^ (x >> 27)) * 0x94d049bb133111eb
	x = x ^ (x >> 31)
	return x
}

// Compile-time assertion that FixtureTagger satisfies Tagger.
var _ Tagger = (*FixtureTagger)(nil)
