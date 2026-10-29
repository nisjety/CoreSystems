package algoliamcp

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestOuterValidationDiagnosticLocatesFixedPropertyWithoutValues(t *testing.T) {
	target := schemaTool(`{"type":"object","properties":{"operationName":{"type":"string","format":"private-format","unknown-assertion":"private-value","enum":["private-operation"]},"arguments":{"type":"object","additionalProperties":{},"$schema":"private-schema"}},"required":["operationName","arguments"]}`)
	shape := outerValidationShape(target, map[string]any{"operationName": listIndices, "arguments": map[string]any{"applicationId": "PRIVATE123"}})
	if !shape.RootSupported || !shape.RequiredKnown || len(shape.Properties) != 2 {
		t.Fatalf("missing validation shape %+v", shape)
	}
	for _, p := range shape.Properties {
		if p.Supported || p.Valid || p.UnknownKeyCount != 1 {
			t.Fatalf("failed constraint not isolated %+v", p)
		}
	}
	encoded, _ := json.Marshal(shape)
	for _, secret := range []string{"private-format", "unknown-assertion", "private-value", "private-operation", "private-schema", "PRIVATE123"} {
		if strings.Contains(string(encoded), secret) {
			t.Fatal("constraint material leaked")
		}
	}
}

func TestOuterConstraintDiagnosticShowsBooleanMatchesNotConstraintValues(t *testing.T) {
	for _, valid := range []bool{false, true} {
		pattern := "^private$"
		enum := "private-operation"
		if valid {
			pattern = "^algolia_"
			enum = listIndices
		}
		raw, _ := json.Marshal(map[string]any{"type": "object", "properties": map[string]any{"operationName": map[string]any{"type": "string", "pattern": pattern, "enum": []string{enum}}, "arguments": map[string]any{"type": "object"}}, "required": []string{"operationName", "arguments"}})
		target := schemaTool(string(raw))
		shape := outerValidationShape(target, map[string]any{"operationName": listIndices, "arguments": map[string]any{}})
		p := shape.Properties[0]
		if shape.Valid != valid || p.EnumContainsTarget != valid || p.PatternMatches != valid || !p.PatternCompiled {
			t.Fatalf("inaccurate constraint flags %+v", shape)
		}
		if !valid && (shape.FirstRejectedKey != "operationName" || shape.RejectionCause != "argument-constraint") {
			t.Fatal("no fixed rejection reason")
		}
		encoded, _ := json.Marshal(shape)
		if strings.Contains(string(encoded), pattern) || strings.Contains(string(encoded), enum) {
			t.Fatal("constraint value leaked")
		}
	}
}
