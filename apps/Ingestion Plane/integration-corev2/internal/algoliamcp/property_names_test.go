package algoliamcp

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

func TestWrappedPropertyNamesChecksActualFixedKeys(t *testing.T) {
	for _, tc := range []struct {
		schema string
		valid  bool
	}{
		{`{"type":"string"}`, true}, {`{"type":"string","pattern":"^applicationId$"}`, true}, {`{"type":"string","pattern":"^private$"}`, false},
		{`{"type":"string","enum":["applicationId"]}`, true}, {`{"type":"string","enum":["private"]}`, false}, {`{"$ref":"#private"}`, false}, {`{"anyOf":[]}`, false}, {`false`, false}, {`true`, true}, {`{}`, true}, {`null`, false},
	} {
		property := `{"type":"object","propertyNames":` + tc.schema + `,"additionalProperties":{}}`
		f := &metaFixture{catalog: strings.Replace(metaCatalog, `"arguments":{"type":"object"}`, `"arguments":`+property, 1), listSchema: unenumeratedApplicationSchema}
		s := metaService(t, f)
		_, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567")
		if (err == nil) != tc.valid || (f.readCalls == 1) != tc.valid {
			t.Fatalf("propertyNames validation wrong %s %v", tc.schema, err)
		}
		shape := argumentsObjectShape(json.RawMessage(property))
		encoded, _ := json.Marshal(shape)
		if strings.Contains(string(encoded), "#private") || strings.Contains(string(encoded), "^private$") {
			t.Fatal("propertyNames assertion leaked")
		}
	}
}

func TestEmptyWrappedObjectVacuouslyPassesPropertyNames(t *testing.T) {
	for _, constraint := range []string{`false`, `{"type":"string","pattern":"^private$"}`, `{"$ref":"#private"}`} {
		target := schemaTool(`{"type":"object","properties":{"arguments":{"type":"object","propertyNames":` + constraint + `,"additionalProperties":{}}},"required":["arguments"]}`)
		if !verifiedArguments(target, map[string]any{"arguments": map[string]any{}}) {
			t.Fatal("empty object evaluated property-name assertion")
		}
	}
}
