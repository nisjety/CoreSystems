// Package search provides utilities for ranking and re-ordering URL sets.
package search

import (
	"context"
	"math"
	"sort"
	"strings"

	"github.com/triodelab/quarry/internal/ai"
)

// EmbeddingClient is the subset of ai.Client needed by the ranker.
type EmbeddingClient interface {
	EmbedText(ctx context.Context, texts []string) ([][]float32, error)
}

// RankByEmbedding reorders urls using semantic similarity to query. It embeds
// the query and all URLs in a single batched call to the AI backend and sorts
// by cosine similarity (descending). The original slice is not mutated.
//
// When the AI backend is unavailable (nil client, error, or unimplemented RPC)
// the function falls back to RankByTFIDF transparently.
func RankByEmbedding(ctx context.Context, client EmbeddingClient, query string, urls []string) ([]string, error) {
	if len(urls) == 0 {
		return urls, nil
	}
	if client == nil {
		return RankByTFIDF(query, urls), nil
	}

	// Batch: query first, then all URLs.
	inputs := make([]string, 0, 1+len(urls))
	inputs = append(inputs, query)
	inputs = append(inputs, urls...)

	embeddings, err := client.EmbedText(ctx, inputs)
	if err != nil || len(embeddings) != len(inputs) {
		// Graceful degradation — fall back to TF-IDF.
		return RankByTFIDF(query, urls), nil
	}

	queryVec := embeddings[0]
	type scored struct {
		url   string
		score float32
	}
	ranked := make([]scored, len(urls))
	for i, u := range urls {
		ranked[i] = scored{url: u, score: cosine32(queryVec, embeddings[1+i])}
	}
	sort.SliceStable(ranked, func(i, j int) bool { return ranked[i].score > ranked[j].score })

	out := make([]string, len(urls))
	for i, s := range ranked {
		out[i] = s.url
	}
	return out, nil
}

// RankByTFIDF ranks urls by simple term overlap with the query using a
// TF-IDF-like weighting. It is used as a fallback when embeddings are
// unavailable and requires no external dependencies.
func RankByTFIDF(query string, urls []string) []string {
	if len(urls) == 0 {
		return urls
	}
	queryTerms := tokenise(query)
	if len(queryTerms) == 0 {
		return urls
	}

	// Build IDF: count how many URLs contain each term.
	df := make(map[string]int, len(queryTerms))
	for _, u := range urls {
		tokens := tokenise(u)
		seen := make(map[string]struct{}, len(tokens))
		for _, t := range tokens {
			if _, ok := seen[t]; !ok {
				df[t]++
				seen[t] = struct{}{}
			}
		}
	}
	n := float64(len(urls))

	type scored struct {
		url   string
		score float64
	}
	ranked := make([]scored, len(urls))
	for i, u := range urls {
		var score float64
		tokens := tokenise(u)
		tf := termFreq(tokens)
		for _, qt := range queryTerms {
			if f, ok := tf[qt]; ok {
				idf := math.Log(n/float64(1+df[qt])) + 1
				score += float64(f) * idf
			}
		}
		ranked[i] = scored{url: u, score: score}
	}
	sort.SliceStable(ranked, func(i, j int) bool { return ranked[i].score > ranked[j].score })

	out := make([]string, len(urls))
	for i, s := range ranked {
		out[i] = s.url
	}
	return out
}

// --- helpers ---

func tokenise(s string) []string {
	s = strings.ToLower(s)
	// Replace non-alphanumeric characters with spaces.
	var b strings.Builder
	for _, r := range s {
		if (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9') {
			b.WriteRune(r)
		} else {
			b.WriteRune(' ')
		}
	}
	fields := strings.Fields(b.String())
	// Remove very short tokens.
	out := fields[:0]
	for _, f := range fields {
		if len(f) >= 2 {
			out = append(out, f)
		}
	}
	return out
}

func termFreq(tokens []string) map[string]int {
	m := make(map[string]int, len(tokens))
	for _, t := range tokens {
		m[t]++
	}
	return m
}

// cosine32 returns the cosine similarity between two float32 vectors.
// Returns 0 when either vector is zero-magnitude.
func cosine32(a, b []float32) float32 {
	if len(a) != len(b) || len(a) == 0 {
		return 0
	}
	var dot, magA, magB float32
	for i := range a {
		dot += a[i] * b[i]
		magA += a[i] * a[i]
		magB += b[i] * b[i]
	}
	if magA == 0 || magB == 0 {
		return 0
	}
	return dot / (float32(math.Sqrt(float64(magA))) * float32(math.Sqrt(float64(magB))))
}

// Ensure the REST AI client satisfies EmbeddingClient at compile time.
var (
	_ EmbeddingClient = (*ai.RESTClient)(nil)
)
