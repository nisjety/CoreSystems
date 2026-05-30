package extractor

import (
	"encoding/json"
	"fmt"
	"strings"
)

// SchemaValidationError describes a single violation of the JSON Schema.
type SchemaValidationError struct {
	Field   string
	Message string
}

func (e SchemaValidationError) Error() string {
	if e.Field != "" {
		return fmt.Sprintf("%s: %s", e.Field, e.Message)
	}
	return e.Message
}

// ValidateAgainstSchema validates extracted JSON data against a JSON Schema
// (draft-07 subset). It checks: required fields, type constraints, and basic
// property existence. Returns a list of violations (empty = valid).
//
// schema must be a JSON Schema object, e.g.:
//
//	{
//	  "type": "object",
//	  "required": ["title", "price"],
//	  "properties": {
//	    "title":  {"type": "string"},
//	    "price":  {"type": "number"},
//	    "inStock": {"type": "boolean"}
//	  }
//	}
func ValidateAgainstSchema(extracted json.RawMessage, schema json.RawMessage) []SchemaValidationError {
	if len(extracted) == 0 || len(schema) == 0 {
		return nil
	}

	// Parse schema.
	var schemaDef map[string]json.RawMessage
	if err := json.Unmarshal(schema, &schemaDef); err != nil {
		return []SchemaValidationError{{Message: "schema parse error: " + err.Error()}}
	}

	// Parse extracted data.
	var data interface{}
	if err := json.Unmarshal(extracted, &data); err != nil {
		return []SchemaValidationError{{Message: "extracted data parse error: " + err.Error()}}
	}

	return validateValue(data, schemaDef, "")
}

func validateValue(value interface{}, schema map[string]json.RawMessage, path string) []SchemaValidationError {
	var errs []SchemaValidationError

	// type check
	if typeRaw, ok := schema["type"]; ok {
		var expectedType string
		if err := json.Unmarshal(typeRaw, &expectedType); err == nil {
			if typeErr := checkType(value, expectedType, path); typeErr != nil {
				errs = append(errs, *typeErr)
				// Stop further validation if the top-level type is wrong.
				if path == "" {
					return errs
				}
			}
		}
	}

	// Only continue with object-level checks if value is actually an object.
	obj, isObj := value.(map[string]interface{})
	if !isObj {
		return errs
	}

	// required fields
	if requiredRaw, ok := schema["required"]; ok {
		var required []string
		if err := json.Unmarshal(requiredRaw, &required); err == nil {
			for _, field := range required {
				if _, present := obj[field]; !present {
					fieldPath := joinPath(path, field)
					errs = append(errs, SchemaValidationError{
						Field:   fieldPath,
						Message: "required field missing",
					})
				}
			}
		}
	}

	// recurse into properties
	if propsRaw, ok := schema["properties"]; ok {
		var props map[string]json.RawMessage
		if err := json.Unmarshal(propsRaw, &props); err == nil {
			for propName, propSchemaRaw := range props {
				propVal, exists := obj[propName]
				if !exists {
					continue // missing non-required fields are OK
				}
				var propSchema map[string]json.RawMessage
				if err := json.Unmarshal(propSchemaRaw, &propSchema); err == nil {
					fieldPath := joinPath(path, propName)
					errs = append(errs, validateValue(propVal, propSchema, fieldPath)...)
				}
			}
		}
	}

	// additionalProperties: false
	if apRaw, ok := schema["additionalProperties"]; ok {
		var ap bool
		if err := json.Unmarshal(apRaw, &ap); err == nil && !ap {
			// collect defined property names
			defined := map[string]struct{}{}
			if propsRaw, ok2 := schema["properties"]; ok2 {
				var props map[string]json.RawMessage
				if err2 := json.Unmarshal(propsRaw, &props); err2 == nil {
					for k := range props {
						defined[k] = struct{}{}
					}
				}
			}
			for k := range obj {
				if _, ok2 := defined[k]; !ok2 {
					errs = append(errs, SchemaValidationError{
						Field:   joinPath(path, k),
						Message: "additional property not allowed",
					})
				}
			}
		}
	}

	// enum check at root
	if enumRaw, ok := schema["enum"]; ok {
		var allowed []interface{}
		if err := json.Unmarshal(enumRaw, &allowed); err == nil {
			matched := false
			vStr := fmt.Sprintf("%v", value)
			for _, a := range allowed {
				if fmt.Sprintf("%v", a) == vStr {
					matched = true
					break
				}
			}
			if !matched {
				errs = append(errs, SchemaValidationError{
					Field:   path,
					Message: fmt.Sprintf("value %q not in allowed enum", vStr),
				})
			}
		}
	}

	return errs
}

// checkType verifies that value matches the expected JSON Schema type string.
func checkType(value interface{}, expectedType, path string) *SchemaValidationError {
	var actual string
	switch value.(type) {
	case nil:
		actual = "null"
	case bool:
		actual = "boolean"
	case float64:
		actual = "number"
	case string:
		actual = "string"
	case []interface{}:
		actual = "array"
	case map[string]interface{}:
		actual = "object"
	default:
		actual = "unknown"
	}

	// JSON Schema "integer" means number with no fractional part.
	if expectedType == "integer" {
		if n, ok := value.(float64); ok && n == float64(int64(n)) {
			return nil
		}
		return &SchemaValidationError{Field: path, Message: fmt.Sprintf("expected integer, got %s", actual)}
	}

	if actual != expectedType {
		return &SchemaValidationError{Field: path, Message: fmt.Sprintf("expected %s, got %s", expectedType, actual)}
	}
	return nil
}

func joinPath(base, field string) string {
	if base == "" {
		return field
	}
	return base + "." + field
}

// ValidationErrorStrings formats errors as a human-readable slice of strings.
func ValidationErrorStrings(errs []SchemaValidationError) []string {
	out := make([]string, 0, len(errs))
	for _, e := range errs {
		out = append(out, e.Error())
	}
	return out
}

// FormatValidationWarning turns schema errors into a single warning string.
func FormatValidationWarning(errs []SchemaValidationError) string {
	if len(errs) == 0 {
		return ""
	}
	msgs := ValidationErrorStrings(errs)
	return "schema validation: " + strings.Join(msgs, "; ")
}
