package search

import "context"

// SearchClient is the common interface for any search provider that can
// execute queries and return uniform Result slices. Implementations include
// BraveClient, GitHubClient, and the Bleve-backed local index.
type SearchClient interface {
	// Search executes a query against the provider and returns results.
	Search(ctx context.Context, searchType SearchType, opts SearchOptions) ([]Result, error)
	// Enabled returns true when the provider is configured and ready.
	Enabled() bool
	// Name returns a human-readable provider name for logging / OTel.
	Name() string
}
