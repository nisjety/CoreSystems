package algoliamcp

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestCatalogDiagnosticOnlyLogsDocumentedLiteralNames(t *testing.T) {
	shape := documentedCatalog([]string{"search", "fetch", "algolia_analytics_top_searches", "private-app-secret-tool", "https://private-token.example"})
	if shape.Total != 5 || shape.Unknown != 2 || len(shape.Documented) != 3 || shape.ListIndicesPresent || shape.NoResultsPresent || shape.SearchCountPresent {
		t.Fatalf("wrong bounded catalog %+v", shape)
	}
	raw, _ := json.Marshal(shape)
	if strings.Contains(string(raw), "private-") || strings.Contains(string(raw), "https://") {
		t.Fatal("unknown tool name leaked")
	}
	shape = documentedCatalog([]string{listIndices, noResults, searchCount, listIndices})
	if shape.Total != 4 || shape.Unknown != 0 || len(shape.Documented) != 3 || !shape.ListIndicesPresent || !shape.NoResultsPresent || !shape.SearchCountPresent {
		t.Fatal("allowlist intersection missing")
	}
}

func TestUnknownOperationDiagnosticExcludesIdentifiersAndPayloads(t *testing.T) {
	shape := documentedCatalog([]string{"search_tools", "get_tool", "execute_tool", "JL96OC7GC2", "eyJhbGciOiJIUzI1NiJ9.payload.signature", "not-safe", "https://private.example/secret", "private\nkey", strings.Repeat("a", 97), "more_than_nine_total_word_parts_are_not_allowed_here_now"})
	if len(shape.UnknownOperations) != 3 {
		t.Fatalf("safe metatool identifiers missing %+v", shape)
	}
	raw, _ := json.Marshal(shape)
	for _, bad := range []string{"JL96OC7GC2", "eyJhbGci", "not-safe", "https://", "private", "more_than"} {
		if strings.Contains(string(raw), bad) {
			t.Fatalf("unsafe operation metadata leaked %s", raw)
		}
	}
	for _, want := range []string{"search_tools", "get_tool", "execute_tool"} {
		if !strings.Contains(string(raw), want) {
			t.Fatal("metatool identifier absent")
		}
	}
	limited := documentedCatalog([]string{"alpha", "beta", "gamma", "delta"})
	if len(limited.UnknownOperations) != 3 {
		t.Fatal("diagnostic name cap missing")
	}
}
