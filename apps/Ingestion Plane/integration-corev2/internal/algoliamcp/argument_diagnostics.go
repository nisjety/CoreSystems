package algoliamcp

import "encoding/json"

type ArgumentsObjectShape struct {
	PropertyNamesPresent               bool     `json:"property_names_present"`
	PropertyNamesKind                  string   `json:"property_names_kind"`
	PropertyNamesType                  string   `json:"property_names_type"`
	PropertyNamesKnownKeys             []string `json:"property_names_known_keys"`
	PropertyNamesKeyCount              int      `json:"property_names_key_count"`
	PropertiesPresent                  bool     `json:"properties_present"`
	PropertiesCount                    int      `json:"properties_count"`
	RequiredCount                      int      `json:"required_count"`
	AdditionalPropertiesKind           string   `json:"additional_properties_kind"`
	AdditionalPropertiesType           string   `json:"additional_properties_type"`
	AdditionalPropertiesKeyCount       int      `json:"additional_properties_key_count"`
	AdditionalPropertiesKnownKeys      []string `json:"additional_properties_known_keys"`
	AdditionalPropertiesAnnotationOnly bool     `json:"additional_properties_annotation_only"`
	ConstraintKeys                     []string `json:"constraint_keys"`
}

func cappedSchemaCount(n int) int {
	if n > 128 {
		return 128
	}
	return n
}
func argumentsObjectShape(raw json.RawMessage) ArgumentsObjectShape {
	fields := shapeObject(raw)
	shape := ArgumentsObjectShape{PropertiesPresent: present(fields, "properties"), PropertiesCount: cappedSchemaCount(len(shapeObject(fields["properties"]))), AdditionalPropertiesKind: dataKind(fields["additionalProperties"]), AdditionalPropertiesType: vettedType(shapeObject(fields["additionalProperties"])["type"]), ConstraintKeys: []string{}}
	shape.PropertyNamesPresent = present(fields, "propertyNames")
	shape.PropertyNamesKind = dataKind(fields["propertyNames"])
	names := shapeObject(fields["propertyNames"])
	shape.PropertyNamesType = vettedType(names["type"])
	shape.PropertyNamesKeyCount = cappedSchemaCount(len(names))
	shape.PropertyNamesKnownKeys = []string{}
	for _, key := range []string{"type", "enum", "const", "$ref", "oneOf", "anyOf", "allOf", "not", "minLength", "maxLength", "pattern", "format", "description", "title", "default", "examples", "deprecated", "$comment", "$schema", "$id", "readOnly", "writeOnly"} {
		if present(names, key) {
			shape.PropertyNamesKnownKeys = append(shape.PropertyNamesKnownKeys, key)
		}
	}
	additional := shapeObject(fields["additionalProperties"])
	shape.AdditionalPropertiesKeyCount = cappedSchemaCount(len(additional))
	shape.AdditionalPropertiesAnnotationOnly = annotationOnlySchema(fields["additionalProperties"])
	shape.AdditionalPropertiesKnownKeys = []string{}
	for _, key := range []string{"type", "enum", "const", "$ref", "oneOf", "anyOf", "allOf", "not", "properties", "required", "items", "additionalProperties", "nullable", "title", "description", "$comment", "default", "examples", "deprecated", "readOnly", "writeOnly", "$schema", "$id", "$defs", "definitions"} {
		if present(additional, key) {
			shape.AdditionalPropertiesKnownKeys = append(shape.AdditionalPropertiesKnownKeys, key)
		}
	}
	var required []json.RawMessage
	if json.Unmarshal(fields["required"], &required) == nil {
		shape.RequiredCount = cappedSchemaCount(len(required))
	}
	for _, key := range []string{"properties", "required", "additionalProperties", "propertyNames", "enum", "$ref", "oneOf", "anyOf", "allOf", "minimum", "maximum", "minProperties", "maxProperties", "patternProperties", "unevaluatedProperties", "dependentRequired", "dependentSchemas", "type", "const", "nullable", "description", "title", "default", "examples", "deprecated", "$comment", "$schema", "$id", "$defs", "definitions", "readOnly", "writeOnly"} {
		if present(fields, key) {
			shape.ConstraintKeys = append(shape.ConstraintKeys, key)
		}
	}
	return shape
}
