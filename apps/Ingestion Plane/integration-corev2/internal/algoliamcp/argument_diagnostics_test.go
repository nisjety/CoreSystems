package algoliamcp

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

func TestMetaReadArgumentFailuresHaveSpecificStages(t *testing.T) {
	for _, tc := range []struct{ property, schema, phase string }{
		{property: `{"type":"object","properties":{"private":{"type":"string"}},"additionalProperties":true}`, phase: "meta-outer-arguments"},
		{schema: strings.Replace(unenumeratedApplicationSchema, `{"type":"string"}`, `{"type":"string","pattern":"^OTHER"}`, 1), phase: "meta-inner-arguments"},
	} {
		f := &metaFixture{listSchema: unenumeratedApplicationSchema}
		if tc.property != "" {
			f.catalog = strings.Replace(metaCatalog, `"arguments":{"type":"object"}`, `"arguments":`+tc.property, 1)
		}
		if tc.schema != "" {
			f.listSchema = tc.schema
		}
		s := metaService(t, f)
		session, tools, done, err := s.open(context.Background(), "org-1", "conn_one")
		if err != nil {
			t.Fatal(err)
		}
		tools, err = session.resolve(context.Background(), tools, listIndices)
		if err != nil {
			t.Fatal(err)
		}
		var events []Diagnostic
		session.diagnostic = func(d Diagnostic) { events = append(events, d) }
		_, err = session.call(context.Background(), listIndices, map[string]any{"applicationId": "APP1234567"})
		done()
		if err == nil || f.readCalls != 0 || len(events) != 1 || events[0].Phase != tc.phase {
			t.Fatalf("ambiguous validation phase %+v %v", events, err)
		}
	}
}

func TestMetaOpenEmptyObjectSchemaSupportsStandardJSONSchemaSemantics(t *testing.T) {
	for _, property := range []string{`{"type":"object","properties":{},"required":[],"additionalProperties":{}}`, `{"type":"object","properties":{},"required":[],"additionalProperties":true}`, `{"type":"object","properties":{},"required":[]}`} {
		f := &metaFixture{catalog: strings.Replace(metaCatalog, `"arguments":{"type":"object"}`, `"arguments":`+property, 1), listSchema: unenumeratedApplicationSchema}
		s := metaService(t, f)
		if items, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567"); err != nil || len(items) != 1 || f.readCalls != 1 {
			t.Fatalf("open object rejected %s %v", property, err)
		}
	}
	for _, property := range []string{`{"type":"object","properties":null}`, `{"type":"object","required":null}`, `{"type":"object","required":["private"]}`, `{"type":"object","properties":{},"additionalProperties":false}`, `{"type":"object","properties":{},"additionalProperties":{"type":"string"}}`, `{"type":"object","properties":{},"anyOf":[]}`} {
		f := &metaFixture{catalog: strings.Replace(metaCatalog, `"arguments":{"type":"object"}`, `"arguments":`+property, 1), listSchema: unenumeratedApplicationSchema}
		s := metaService(t, f)
		if _, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567"); err == nil || f.readCalls != 0 {
			t.Fatalf("unproved object accepted %s", property)
		}
	}
}

func TestAnnotationOnlyAdditionalPropertiesIsUnconstrainedButAssertionsAreNot(t *testing.T) {
	for _, tc := range []struct {
		additional string
		valid      bool
	}{
		{`{"description":"private-annotation","default":null,"examples":[],"deprecated":false}`, true},
		{`{"title":"private-annotation","$comment":"private-comment","readOnly":true,"writeOnly":false}`, true},
		{`{"anyOf":[]}`, false}, {`{"$ref":"private-reference"}`, false}, {`{"unknown":"private"}`, false}, {`{"type":"string"}`, false}, {`{"description":42}`, false},
	} {
		property := `{"type":"object","additionalProperties":` + tc.additional + `}`
		f := &metaFixture{catalog: strings.Replace(metaCatalog, `"arguments":{"type":"object"}`, `"arguments":`+property, 1), listSchema: unenumeratedApplicationSchema}
		s := metaService(t, f)
		_, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567")
		if (err == nil) != tc.valid || (f.readCalls == 1) != tc.valid {
			t.Fatalf("annotation proof wrong for %s: %v", tc.additional, err)
		}
		shape := argumentsObjectShape(json.RawMessage(property))
		encoded, _ := json.Marshal(shape)
		for _, secret := range []string{"private-annotation", "private-comment", "private-reference"} {
			if strings.Contains(string(encoded), secret) {
				t.Fatal("annotation material leaked")
			}
		}
	}
}

func TestEmptyWrappedArgumentsDoNotEvaluateAdditionalPropertySchema(t *testing.T) {
	for _, additional := range []string{`false`, `{"type":"string"}`, `{"$ref":"#/$defs/provider"}`} {
		typeSchema := schemaTool(`{"type":"object","properties":{"arguments":{"type":"object","properties":{},"required":[],"additionalProperties":` + additional + `}},"required":["arguments"]}`)
		if !verifiedArguments(typeSchema, map[string]any{"arguments": map[string]any{}}) {
			t.Fatal("empty object incorrectly evaluated additional-property assertion")
		}
		if verifiedArguments(typeSchema, map[string]any{"arguments": map[string]any{"applicationId": "APP1234567"}}) {
			t.Fatal("nonempty object bypassed additional-property constraint")
		}
	}
}

func TestArgumentsObjectDiagnosticNeverProjectsRawSchemaValues(t *testing.T) {
	raw := json.RawMessage(`{"type":"object","properties":{"private-property":{"description":"private-token"}},"required":["private-property"],"additionalProperties":{"type":"string","description":"private-token"},"$ref":"private-ref","allOf":[{"private":"value"}]}`)
	shape := argumentsObjectShape(raw)
	if !shape.PropertiesPresent || shape.PropertiesCount != 1 || shape.RequiredCount != 1 || shape.AdditionalPropertiesKind != "object" || shape.AdditionalPropertiesType != "string" {
		t.Fatalf("missing bounded flags %+v", shape)
	}
	encoded, _ := json.Marshal(shape)
	for _, forbidden := range []string{"private-property", "private-token", "private-ref", "value"} {
		if strings.Contains(string(encoded), forbidden) {
			t.Fatal("schema material leaked")
		}
	}
}
