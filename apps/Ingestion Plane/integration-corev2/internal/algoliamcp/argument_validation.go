package algoliamcp

import (
	"bytes"
	"encoding/json"
	"regexp"
	"time"
)

func onlySchemaKeys(fields map[string]json.RawMessage, allowed map[string]bool) bool {
	if fields == nil {
		return false
	}
	for key := range fields {
		if !allowed[key] {
			return false
		}
	}
	return true
}
func supportedRoot(t tool) bool {
	root := shapeObject(t.RawSchema)
	return onlySchemaKeys(root, map[string]bool{"type": true, "properties": true, "required": true, "additionalProperties": true, "title": true, "description": true, "$schema": true, "$id": true, "$defs": true, "definitions": true, "examples": true, "default": true}) && vettedType(root["type"]) == "object"
}
func supportedProperty(fields map[string]json.RawMessage) bool {
	return onlySchemaKeys(fields, map[string]bool{"type": true, "enum": true, "description": true, "title": true, "default": true, "examples": true, "deprecated": true, "minimum": true, "maximum": true, "minLength": true, "maxLength": true, "pattern": true, "format": true, "additionalProperties": true})
}

// An empty object schema is open by JSON Schema semantics. The fixed inner
// operation schema has already validated every actual argument before wrapping.
// Nonempty object constraints are not silently ignored or generalized.
func supportedOpenObject(fields map[string]json.RawMessage, value map[string]any) bool {
	if !onlySchemaKeys(fields, map[string]bool{"type": true, "properties": true, "required": true, "additionalProperties": true, "propertyNames": true, "description": true, "title": true, "default": true, "examples": true, "deprecated": true}) || vettedType(fields["type"]) != "object" {
		return false
	}
	if raw, ok := fields["properties"]; ok {
		object := shapeObject(raw)
		if object == nil || len(object) != 0 {
			return false
		}
	}
	if raw, ok := fields["required"]; ok {
		var required []string
		if json.Unmarshal(raw, &required) != nil || required == nil || len(required) != 0 {
			return false
		}
	}
	if raw, ok := fields["propertyNames"]; ok && !verifiedPropertyNames(raw, value) {
		return false
	}
	if raw, ok := fields["additionalProperties"]; ok {
		// No additional-property assertion is evaluated for the fixed empty
		// application-list argument object. Nonempty objects remain strict.
		if len(value) == 0 && (bytes.Equal(bytes.TrimSpace(raw), []byte("false")) || shapeObject(raw) != nil) {
			return true
		}
		if bytes.Equal(bytes.TrimSpace(raw), []byte("true")) {
			return true
		}
		return annotationOnlySchema(raw)
	}
	return true
}

func verifiedPropertyNames(raw json.RawMessage, value map[string]any) bool {
	trimmed := bytes.TrimSpace(raw)
	if bytes.Equal(trimmed, []byte("true")) {
		return true
	}
	if bytes.Equal(trimmed, []byte("false")) {
		return len(value) == 0
	}
	if shapeObject(raw) == nil {
		return false
	}
	// The assertion applies once per actual key; no keys means no evaluation.
	if len(value) == 0 || annotationOnlySchema(raw) {
		return true
	}
	var parsed property
	if json.Unmarshal(raw, &parsed) != nil || parsed.Type != "string" {
		return false
	}
	root, _ := json.Marshal(map[string]any{"type": "object", "properties": map[string]json.RawMessage{"key": raw}, "required": []string{"key"}})
	input := tool{RawSchema: root, InputSchema: schema{Type: "object", Properties: map[string]property{"key": parsed}, Required: []string{"key"}}}
	for key := range value {
		if !verifiedArguments(input, map[string]any{"key": key}) {
			return false
		}
	}
	return true
}

func annotationOnlySchema(raw json.RawMessage) bool {
	fields := shapeObject(raw)
	if !onlySchemaKeys(fields, map[string]bool{"title": true, "description": true, "$comment": true, "default": true, "examples": true, "deprecated": true, "readOnly": true, "writeOnly": true}) {
		return false
	}
	for key, value := range fields {
		switch key {
		case "title", "description", "$comment":
			var text string
			if json.Unmarshal(value, &text) != nil || dataKind(value) != "string" {
				return false
			}
		case "examples":
			var entries []json.RawMessage
			if json.Unmarshal(value, &entries) != nil || entries == nil {
				return false
			}
		case "deprecated", "readOnly", "writeOnly":
			var flag bool
			if json.Unmarshal(value, &flag) != nil || dataKind(value) != "boolean" {
				return false
			}
		}
	}
	return true
}
func numericConstraint(fields map[string]json.RawMessage, key string, value float64, minimum bool) bool {
	raw, ok := fields[key]
	if !ok {
		return true
	}
	if bytes.Equal(bytes.TrimSpace(raw), []byte("null")) {
		return false
	}
	var bound float64
	if json.Unmarshal(raw, &bound) != nil {
		return false
	}
	if minimum {
		return value >= bound
	}
	return value <= bound
}
func verifiedArguments(t tool, args map[string]any) bool {
	if !supportedRoot(t) {
		return false
	}
	allowed := map[string]bool{}
	for name := range args {
		allowed[name] = true
	}
	if !knownRequired(t.InputSchema, allowed) {
		return false
	}
	props := shapeObject(shapeObject(t.RawSchema)["properties"])
	for name, value := range args {
		p, exists := t.InputSchema.Properties[name]
		fields := shapeObject(props[name])
		objectValue, objectArgument := value.(map[string]any)
		if !exists || (objectArgument && !supportedOpenObject(fields, objectValue)) || (!objectArgument && !supportedProperty(fields)) {
			return false
		}
		if raw, ok := fields["enum"]; ok {
			var entries []string
			if json.Unmarshal(raw, &entries) != nil || entries == nil || len(entries) == 0 {
				return false
			}
			seen := map[string]bool{}
			for _, entry := range entries {
				if seen[entry] {
					return false
				}
				seen[entry] = true
			}
		}
		switch value := value.(type) {
		case string:
			if p.Type != "string" || !numericConstraint(fields, "minLength", float64(len(value)), true) || !numericConstraint(fields, "maxLength", float64(len(value)), false) {
				return false
			}
			if raw, ok := fields["pattern"]; ok {
				var pattern string
				if len(raw) == 0 || raw[0] != '"' || json.Unmarshal(raw, &pattern) != nil || len(pattern) > 256 {
					return false
				}
				compiled, err := regexp.Compile(pattern)
				if err != nil || !compiled.MatchString(value) {
					return false
				}
			}
			if raw, ok := fields["format"]; ok {
				var format string
				if len(raw) == 0 || raw[0] != '"' || json.Unmarshal(raw, &format) != nil || format != "date" {
					return false
				}
				if _, err := time.Parse(time.DateOnly, value); err != nil {
					return false
				}
			}
			if len(p.Enum) > 0 {
				match := false
				for _, literal := range p.Enum {
					match = match || literal == value
				}
				if !match {
					return false
				}
			}
		case int:
			if p.Type != "integer" || !numericConstraint(fields, "minimum", float64(value), true) || !numericConstraint(fields, "maximum", float64(value), false) || len(p.Enum) > 0 {
				return false
			}
		case map[string]any:
			if p.Type != "object" || len(p.Enum) > 0 {
				return false
			}
		default:
			return false
		}
	}
	return true
}
