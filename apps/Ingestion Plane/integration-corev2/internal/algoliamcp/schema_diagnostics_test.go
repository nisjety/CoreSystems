package algoliamcp

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestApplicationShapeDiagnosticNeverIncludesRawSchemaValues(t *testing.T) {
	raw := json.RawMessage(`{"type":"object","description":"secret-description","properties":{"applicationId":{"type":"string","enum":["APP1234567"],"anyOf":[{"$ref":"https://secret.example/private"}]}},"required":["applicationId"],"$ref":"#/$defs/hidden"}`)
	shape := applicationShape(map[string]tool{listIndices: {RawSchema: raw}})
	if !shape.ToolPresent || shape.SchemaType != "object" || !shape.AppPropertyPresent || shape.AppType != "string" || !shape.EnumPresent || !shape.EnumArray || shape.EnumCount != 1 || !shape.EnumStrings || !shape.EnumValid || !shape.RequiredKnown || !shape.RequiredApplication || !shape.AppAnyOf || !shape.SchemaRef {
		t.Fatalf("incorrect sanitized shape %+v", shape)
	}
	data, _ := json.Marshal(shape)
	for _, bad := range []string{"APP1234567", "secret", "hidden", "https://", "description"} {
		if strings.Contains(string(data), bad) {
			t.Fatalf("raw schema data leaked: %s", data)
		}
	}
}
func TestApplicationShapeHandlesMissingAndCompositeTypes(t *testing.T) {
	absent := applicationShape(nil)
	if absent.ToolPresent || absent.AppPropertyPresent {
		t.Fatal("missing tool fabricated")
	}
	raw := json.RawMessage(`{"type":"secret-schema-type","properties":{"applicationId":{"type":["string","null"],"oneOf":[],"$ref":"#/$defs/application","enum":[1,"APP1234567"]}},"required":["applicationId","private-required"]}`)
	shape := applicationShape(map[string]tool{listIndices: {RawSchema: raw}})
	if shape.SchemaType != "other" || shape.AppType != "union" || shape.EnumStrings || shape.EnumValid || shape.RequiredKnown || !shape.AppOneOf || !shape.AppRef {
		t.Fatalf("unsafe composite shape %+v", shape)
	}
	data, _ := json.Marshal(shape)
	if strings.Contains(string(data), "secret-schema") || strings.Contains(string(data), "private-required") {
		t.Fatal("raw type or property leaked")
	}
}
