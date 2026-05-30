package ai

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestDereferenceSchema_BasicDefs(t *testing.T) {
	schema := `{
		"type": "object",
		"properties": {
			"address": {"$ref": "#/$defs/Address"}
		},
		"$defs": {
			"Address": {
				"type": "object",
				"properties": {
					"street": {"type": "string"}
				}
			}
		}
	}`

	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if strings.Contains(result, "$ref") {
		t.Fatalf("$ref not resolved: %s", result)
	}
	if !strings.Contains(result, `"street"`) {
		t.Fatalf("expected inlined address properties: %s", result)
	}
}

func TestDereferenceSchema_Definitions(t *testing.T) {
	schema := `{
		"type": "object",
		"properties": {
			"address": {"$ref": "#/definitions/Address"}
		},
		"definitions": {
			"Address": {
				"type": "object",
				"properties": {
					"city": {"type": "string"}
				}
			}
		}
	}`

	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if strings.Contains(result, "$ref") {
		t.Fatalf("$ref not resolved: %s", result)
	}
	if !strings.Contains(result, `"city"`) {
		t.Fatalf("expected inlined definition: %s", result)
	}
}

func TestDereferenceSchema_NestedPointer(t *testing.T) {
	schema := `{
		"type": "object",
		"properties": {
			"item": {"$ref": "#/$defs/nested/Item"}
		},
		"$defs": {
			"nested": {
				"Item": {
					"type": "string"
				}
			}
		}
	}`

	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if strings.Contains(result, "$ref") {
		t.Fatalf("nested $ref not resolved: %s", result)
	}
}

func TestDereferenceSchema_CircularDetection(t *testing.T) {
	schema := `{
		"type": "object",
		"properties": {
			"self": {"$ref": "#/$defs/A"}
		},
		"$defs": {
			"A": {"$ref": "#/$defs/B"},
			"B": {"$ref": "#/$defs/A"}
		}
	}`

	// Should gracefully return original schema (no panic, no infinite loop).
	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// The result should be the original schema since circular ref causes graceful fallback.
	if result == "" {
		t.Fatal("expected non-empty result")
	}
}

func TestDereferenceSchema_ExternalRefUntouched(t *testing.T) {
	schema := `{
		"type": "object",
		"properties": {
			"external": {"$ref": "https://example.com/schema.json"}
		}
	}`

	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(result, "https://example.com/schema.json") {
		t.Fatalf("external $ref should remain unchanged: %s", result)
	}
}

func TestDereferenceSchema_EmptyInput(t *testing.T) {
	result, err := DereferenceSchema("")
	if err != nil {
		t.Fatal(err)
	}
	if result != "" {
		t.Fatalf("expected empty, got: %s", result)
	}
}

func TestDereferenceSchema_NoRefs(t *testing.T) {
	schema := `{"type": "string"}`
	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatal(err)
	}
	if result != schema {
		t.Fatalf("expected unchanged, got: %s", result)
	}
}

func TestDereferenceSchema_SiblingKeysMerged(t *testing.T) {
	schema := `{
		"type": "object",
		"properties": {
			"name": {
				"$ref": "#/$defs/NameType",
				"description": "Full name"
			}
		},
		"$defs": {
			"NameType": {"type": "string", "minLength": 1}
		}
	}`
	result, err := DereferenceSchema(schema)
	if err != nil {
		t.Fatal(err)
	}
	var parsed map[string]interface{}
	if err := json.Unmarshal([]byte(result), &parsed); err != nil {
		t.Fatal(err)
	}
	props := parsed["properties"].(map[string]interface{})
	nameField := props["name"].(map[string]interface{})
	if nameField["description"] != "Full name" {
		t.Fatal("sibling description key should be preserved")
	}
	if nameField["type"] != "string" {
		t.Fatal("type from $ref should be inlined")
	}
}

func TestValidateSchema_ValidPayload(t *testing.T) {
	schema := `{"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}`
	payload := `{"name": "test"}`
	if err := ValidateSchema(schema, payload); err != nil {
		t.Fatalf("expected valid, got: %v", err)
	}
}

func TestValidateSchema_InvalidPayload(t *testing.T) {
	schema := `{"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}`
	payload := `{"age": 42}`
	if err := ValidateSchema(schema, payload); err == nil {
		t.Fatal("expected validation error for missing required field")
	}
}

func TestIsSchemaValid(t *testing.T) {
	tests := []struct {
		name  string
		input string
		want  bool
	}{
		{"valid", `{"type": "object"}`, true},
		{"empty", "", false},
		{"bad json", `{not json}`, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := IsSchemaValid(tt.input); got != tt.want {
				t.Errorf("IsSchemaValid(%q) = %v, want %v", tt.input, got, tt.want)
			}
		})
	}
}
