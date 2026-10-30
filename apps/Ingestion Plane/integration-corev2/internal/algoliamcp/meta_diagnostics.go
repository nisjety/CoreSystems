package algoliamcp

import (
	"encoding/json"
	"sort"
)

type MetaArgumentShape struct {
	Name    string `json:"name"`
	Present bool   `json:"present"`
	Type    string `json:"type"`
}
type MetaOuterShape struct {
	Tool           string              `json:"tool"`
	Present        bool                `json:"present"`
	Type           string              `json:"type"`
	RequiredKnown  bool                `json:"required_known"`
	PromptRequired bool                `json:"prompt_required"`
	Arguments      []MetaArgumentShape `json:"arguments"`
}
type MetaShape struct {
	Operation        string                `json:"operation"`
	KnownKeys        []string              `json:"known_keys"`
	Entries          int                   `json:"entries"`
	OperationMatches bool                  `json:"operation_matches"`
	EffectRead       bool                  `json:"effect_read"`
	SchemaPresent    bool                  `json:"schema_present"`
	Outer            []MetaOuterShape      `json:"outer"`
	ArgumentsObject  *ArgumentsObjectShape `json:"arguments_object,omitempty"`
	OuterValidation  *OuterValidationShape `json:"outer_validation,omitempty"`
	ReportInput      *ReportInputShape     `json:"report_input,omitempty"`
	ReportResult     *ReportResultShape    `json:"report_result,omitempty"`
}

func (s *session) metaFailure(stage, operation string, raw json.RawMessage, tools map[string]tool, validation ...OuterValidationShape) error {
	if s.diagnostic == nil {
		return ErrUnavailable
	}
	shape := MetaShape{Operation: operation, KnownKeys: []string{}, Outer: []MetaOuterShape{}}
	if len(validation) == 1 {
		shape.OuterValidation = &validation[0]
	}
	if operation == topSearches || operation == searchesNoResults || operation == noResults {
		input := reportInputShape(tools[operation])
		shape.ReportInput = &input
		if len(raw) > 0 {
			result := reportResultShape(raw)
			shape.ReportResult = &result
		}
	}
	fields := metadataObject(raw)
	known := []string{"tools", "operations", "results", "operationName", "name", "effect", "inputSchema", "schema", "parameters", "operation", "tool", "result", "data", "response", "hasMore", "nextCursor", "count", "noResultCount", "rate", "dates", "items"}
	for _, key := range known {
		if present(fields, key) {
			shape.KnownKeys = append(shape.KnownKeys, key)
		}
	}
	sort.Strings(shape.KnownKeys)
	shape.OperationMatches = operationMatches(fields, operation)
	shape.EffectRead = effectRead(fields)
	shape.SchemaPresent = present(fields, "inputSchema") || present(fields, "schema") || present(fields, "parameters")
	if entries, ok := discoveryEntries(raw); ok {
		shape.Entries = len(entries)
		for _, entry := range entries {
			candidate := metadataObject(entry)
			if operationMatches(candidate, operation) {
				shape.OperationMatches = true
				shape.EffectRead = effectRead(candidate)
				shape.SchemaPresent = present(candidate, "inputSchema") || present(candidate, "schema") || present(candidate, "parameters")
			}
		}
	}
	for _, name := range []string{discoverTools, describeTool, callReadTool} {
		t, exists := tools[name]
		outer := MetaOuterShape{Tool: name, Present: exists, Type: vettedType(shapeObject(t.RawSchema)["type"]), Arguments: []MetaArgumentShape{}}
		allowed := map[string]bool{"operationName": true}
		if name == discoverTools {
			allowed = map[string]bool{"query": true, "limit": true, "offset": true}
		} else if name == callReadTool {
			allowed["arguments"] = true
		}
		outer.RequiredKnown = knownRequired(t.InputSchema, allowed)
		props := shapeObject(shapeObject(t.RawSchema)["properties"])
		if name == callReadTool {
			objectShape := argumentsObjectShape(props["arguments"])
			shape.ArgumentsObject = &objectShape
		}
		for _, argument := range []string{"query", "limit", "offset", "operationName", "arguments", "originalQuery", "userIntent"} {
			p, ok := props[argument]
			outer.Arguments = append(outer.Arguments, MetaArgumentShape{Name: argument, Present: ok, Type: vettedType(shapeObject(p)["type"])})
		}
		for _, required := range t.InputSchema.Required {
			outer.PromptRequired = outer.PromptRequired || required == "originalQuery" || required == "userIntent"
		}
		shape.Outer = append(shape.Outer, outer)
	}
	s.diagnostic(Diagnostic{Phase: stage, HTTPStatus: 200, MetaShape: &shape})
	return ErrUnavailable
}
