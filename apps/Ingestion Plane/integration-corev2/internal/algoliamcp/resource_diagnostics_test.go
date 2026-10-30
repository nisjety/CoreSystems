package algoliamcp

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestResourceShapeExposesOnlyFixedPathPresenceAndValidity(t *testing.T) {
	raw := json.RawMessage(`{"context":{"applications":[{"id":"APP1234567","name":"private-name"}]},"private_payload":"secret-value","operation":{"operationName":"algolia_search_list_indices","effect":"read","availableApplications":["OTHER12345"],"inputSchema":{"type":"object","properties":{"applicationId":{"type":"string","default":"APP1234567"}}}}}`)
	result := resourceShape(raw)
	if len(result.Paths) != 2 || !result.AppDefaultPresent {
		t.Fatalf("missing fixed-path projections %+v", result)
	}
	for _, path := range result.Paths {
		if path.Count != 1 || !path.IdentifiersValid {
			t.Fatal("typed identifier presence not summarized")
		}
	}
	encoded, _ := json.Marshal(result)
	for _, private := range []string{"APP1234567", "OTHER12345", "private-name", "private_payload", "secret-value"} {
		if strings.Contains(string(encoded), private) {
			t.Fatal("resource values leaked")
		}
	}
	invalid := resourceShape(json.RawMessage(`{"applications":["https://private.example/secret"]}`))
	if len(invalid.Paths) != 1 || invalid.Paths[0].IdentifiersValid {
		t.Fatal("unsafe app identifiers appeared valid")
	}
	absent := resourceShape(json.RawMessage(`{"description":"no machine-readable list"}`))
	if len(absent.Paths) != 0 {
		t.Fatal("metadata list fabricated")
	}
}
