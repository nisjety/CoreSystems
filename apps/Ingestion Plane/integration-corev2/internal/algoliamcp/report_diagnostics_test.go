package algoliamcp

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestReportDiagnosticsProjectOnlyFixedTypesAndCounts(t *testing.T) {
	tool := schemaTool(`{"type":"object","properties":{"applicationId":{"type":"string","enum":["PRIVATE123"]},"index":{"type":"string"},"limit":{"type":"integer"},"private-property":{"type":"string"}},"required":["applicationId","private-property"]}`)
	inner := reportInputShape(tool)
	if inner.PropertiesCount != 4 || inner.RequiredKnown {
		t.Fatal("missing safe input proof")
	}
	result := reportResultShape(json.RawMessage(`{"searches":[{"search":"private-term","count":99,"nbHits":2,"userId":"private-user"}],"private-key":"private-value"}`))
	if result.RootKeyCount != 2 || len(result.Arrays) != 1 || result.Arrays[0].Count != 1 {
		t.Fatal("missing result shape")
	}
	encoded, _ := json.Marshal([]any{inner, result})
	for _, secret := range []string{"PRIVATE123", "private-property", "private-term", "private-user", "private-key", "private-value"} {
		if strings.Contains(string(encoded), secret) {
			t.Fatal("private report/schema material logged")
		}
	}
}
