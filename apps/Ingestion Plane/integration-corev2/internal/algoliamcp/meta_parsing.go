package algoliamcp

import (
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/webvitals"
)

func metadataObject(raw json.RawMessage) map[string]json.RawMessage {
	fields := shapeObject(raw)
	if fields == nil {
		return nil
	}
	if present(fields, "operationName") || present(fields, "name") {
		return fields
	}
	// Recognized provider metadata envelopes only, one level, never a JSONPath.
	var nested json.RawMessage
	for _, key := range []string{"operation", "tool", "result", "data"} {
		if value, ok := fields[key]; ok {
			if nested != nil {
				return nil
			}
			nested = value
		}
	}
	if nested != nil {
		return shapeObject(nested)
	}
	return fields
}
func operationMatches(fields map[string]json.RawMessage, operation string) bool {
	matched := false
	for _, key := range []string{"operationName", "name"} {
		if raw, ok := fields[key]; ok {
			var name string
			if json.Unmarshal(raw, &name) != nil || name != operation {
				return false
			}
			matched = true
		}
	}
	return matched
}
func effectRead(fields map[string]json.RawMessage) bool {
	var effect string
	return json.Unmarshal(fields["effect"], &effect) == nil && effect == "read"
}
func discoveryEntries(raw json.RawMessage) ([]json.RawMessage, bool) {
	if len(raw) > maxResponse || webvitals.UniqueJSON(raw) != nil {
		return nil, false
	}
	var entries []json.RawMessage
	if json.Unmarshal(raw, &entries) == nil && entries != nil {
		return entries, len(entries) <= 32
	}
	fields := shapeObject(raw)
	if fields == nil {
		return nil, false
	}
	if cursor, ok := fields["nextCursor"]; ok {
		var next string
		if json.Unmarshal(cursor, &next) != nil || next != "" {
			return nil, false
		}
	}
	if more, ok := fields["hasMore"]; ok {
		var next bool
		if json.Unmarshal(more, &next) != nil || next {
			return nil, false
		}
	}
	var payload json.RawMessage
	for _, key := range []string{"tools", "operations", "results"} {
		if value, ok := fields[key]; ok {
			if payload != nil {
				return nil, false
			}
			payload = value
		}
	}
	if payload == nil {
		return nil, false
	}
	if json.Unmarshal(payload, &entries) != nil || entries == nil || len(entries) > 32 {
		return nil, false
	}
	return entries, true
}
func discoveryRead(raw json.RawMessage, operation string) bool {
	entries, ok := discoveryEntries(raw)
	if !ok {
		return false
	}
	matches := 0
	for _, entry := range entries {
		fields := metadataObject(entry)
		if operationMatches(fields, operation) {
			if !effectRead(fields) {
				return false
			}
			matches++
		}
	}
	return matches == 1
}
func describedRead(raw json.RawMessage, operation string) (tool, bool) {
	if len(raw) > maxResponse || webvitals.UniqueJSON(raw) != nil {
		return tool{}, false
	}
	fields := metadataObject(raw)
	if !operationMatches(fields, operation) || !effectRead(fields) {
		return tool{}, false
	}
	var input json.RawMessage
	for _, key := range []string{"inputSchema", "schema", "parameters"} {
		if value, ok := fields[key]; ok {
			if input != nil {
				return tool{}, false
			}
			input = value
		}
	}
	var parsed schema
	if input == nil || json.Unmarshal(input, &parsed) != nil || parsed.Type != "object" {
		return tool{}, false
	}
	described := tool{Name: operation, InputSchema: parsed, RawSchema: input, ResourceMetadata: raw}
	if !supportedRoot(described) {
		return tool{}, false
	}
	return described, true
}
func readPayload(raw json.RawMessage) json.RawMessage {
	fields := shapeObject(raw)
	if len(fields) == 1 {
		if result, ok := fields["result"]; ok {
			return result
		}
		if data, ok := fields["data"]; ok {
			return data
		}
	}
	return raw
}
