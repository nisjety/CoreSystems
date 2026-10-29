package algoliamcp

import (
	"encoding/json"
	"regexp"
)

type OuterPropertyValidationShape struct {
	Name               string   `json:"name"`
	Present            bool     `json:"present"`
	Type               string   `json:"type"`
	Supported          bool     `json:"supported"`
	Valid              bool     `json:"valid"`
	ConstraintKeys     []string `json:"constraint_keys"`
	UnknownKeyCount    int      `json:"unknown_key_count"`
	EnumPresent        bool     `json:"enum_present"`
	EnumArray          bool     `json:"enum_array"`
	EnumCount          int      `json:"enum_count"`
	EnumContainsTarget bool     `json:"enum_contains_target"`
	FormatKind         string   `json:"format_kind"`
	PatternKind        string   `json:"pattern_kind"`
	PatternCompiled    bool     `json:"pattern_compiled"`
	PatternMatches     bool     `json:"pattern_matches"`
}
type OuterValidationShape struct {
	RootSupported    bool                           `json:"root_supported"`
	RequiredKnown    bool                           `json:"required_known"`
	Valid            bool                           `json:"valid"`
	Properties       []OuterPropertyValidationShape `json:"properties"`
	FirstRejectedKey string                         `json:"first_rejected_key,omitempty"`
	RejectionCause   string                         `json:"rejection_cause,omitempty"`
}

// Only fixed provided argument names and assertion presence/kinds are visible.
// Inputs, enum members, patterns, formats and unknown keyword names stay private.
func outerValidationShape(t tool, args map[string]any) OuterValidationShape {
	shape := OuterValidationShape{RootSupported: supportedRoot(t), Valid: verifiedArguments(t, args), Properties: []OuterPropertyValidationShape{}}
	allowed := map[string]bool{}
	for name := range args {
		allowed[name] = true
	}
	shape.RequiredKnown = knownRequired(t.InputSchema, allowed)
	props := shapeObject(shapeObject(t.RawSchema)["properties"])
	for _, name := range []string{"operationName", "arguments"} {
		fields := shapeObject(props[name])
		_, present := props[name]
		p := OuterPropertyValidationShape{Name: name, Present: present, Type: vettedType(fields["type"]), ConstraintKeys: []string{}, FormatKind: "missing", PatternKind: dataKind(fields["pattern"])}
		p.Supported = supportedProperty(fields)
		object, objectArgument := args[name].(map[string]any)
		if objectArgument {
			p.Supported = supportedOpenObject(fields, object)
		}
		knownAllowed := map[string]bool{}
		for _, key := range []string{"type", "enum", "description", "title", "default", "examples", "deprecated", "minimum", "maximum", "minLength", "maxLength", "pattern", "format", "additionalProperties"} {
			knownAllowed[key] = true
		}
		if objectArgument {
			knownAllowed = map[string]bool{"type": true, "properties": true, "required": true, "additionalProperties": true, "propertyNames": true, "description": true, "title": true, "default": true, "examples": true, "deprecated": true}
		}
		for key := range fields {
			if !knownAllowed[key] {
				p.UnknownKeyCount++
			}
		}
		p.UnknownKeyCount = cappedSchemaCount(p.UnknownKeyCount)
		for _, key := range []string{"type", "enum", "minimum", "maximum", "minLength", "maxLength", "pattern", "format", "properties", "required", "additionalProperties", "propertyNames", "$ref", "oneOf", "anyOf", "allOf", "not", "const", "$schema", "$id", "nullable", "readOnly", "writeOnly", "$comment", "$defs", "definitions", "description", "title", "default", "examples", "deprecated", "minProperties", "maxProperties", "patternProperties", "unevaluatedProperties", "dependentRequired", "dependentSchemas"} {
			if _, ok := fields[key]; ok {
				p.ConstraintKeys = append(p.ConstraintKeys, key)
			}
		}
		if raw, ok := fields["enum"]; ok {
			p.EnumPresent = true
			var entries []json.RawMessage
			p.EnumArray = json.Unmarshal(raw, &entries) == nil && entries != nil
			p.EnumCount = cappedSchemaCount(len(entries))
			if target, ok := args[name].(string); ok {
				for _, entry := range entries {
					var candidate string
					if json.Unmarshal(entry, &candidate) == nil && candidate == target {
						p.EnumContainsTarget = true
					}
				}
			}
		}
		if raw, ok := fields["format"]; ok {
			var format string
			p.FormatKind = "unsupported"
			if json.Unmarshal(raw, &format) == nil && format == "date" {
				p.FormatKind = "date"
			}
		}
		if raw, ok := fields["pattern"]; ok {
			var pattern string
			if dataKind(raw) == "string" && len(raw) <= 1024 && json.Unmarshal(raw, &pattern) == nil && len(pattern) <= 256 {
				compiled, err := regexp.Compile(pattern)
				if err == nil {
					p.PatternCompiled = true
					if target, ok := args[name].(string); ok {
						p.PatternMatches = compiled.MatchString(target)
					}
				}
			}
		}
		raw, _ := json.Marshal(map[string]any{"type": "object", "properties": map[string]any{name: props[name]}, "required": []string{name}})
		single := tool{RawSchema: raw, InputSchema: schema{Type: "object", Properties: map[string]property{name: t.InputSchema.Properties[name]}, Required: []string{name}}}
		p.Valid = verifiedArguments(single, map[string]any{name: args[name]})
		shape.Properties = append(shape.Properties, p)
	}
	if !shape.RootSupported {
		shape.FirstRejectedKey = "root"
		shape.RejectionCause = "unsupported-schema"
	} else if !shape.RequiredKnown {
		shape.FirstRejectedKey = "root"
		shape.RejectionCause = "required-arguments"
	} else {
		for _, p := range shape.Properties {
			if !p.Supported {
				shape.FirstRejectedKey = p.Name
				shape.RejectionCause = "unsupported-schema"
				break
			}
			if !p.Valid {
				shape.FirstRejectedKey = p.Name
				shape.RejectionCause = "argument-constraint"
				break
			}
		}
	}
	return shape
}
