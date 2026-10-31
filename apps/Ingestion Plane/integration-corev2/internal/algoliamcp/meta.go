package algoliamcp

import (
	"context"
	"encoding/json"
)

const discoverTools = "algolia_discover_tools"
const describeTool = "algolia_describe_tool"
const callReadTool = "algolia_call_read_tool"
const topSearches = "algolia_analytics_top_searches"
const searchesNoResults = "algolia_analytics_searches_no_results"

func allowedLeaf(name string) bool {
	return name == listIndices || name == noResults || name == searchCount || name == applicationList || name == topSearches || name == searchesNoResults
}
func requiresDescription(name string) bool {
	return name == applicationList || name == topSearches || name == searchesNoResults
}
func allowedMeta(name string) bool {
	return name == discoverTools || name == describeTool || name == callReadTool
}
func metaSchema(t tool, allowed map[string]string) bool {
	if t.InputSchema.Type != "object" || !supportedRoot(t) {
		return false
	}
	required := map[string]bool{}
	for name := range allowed {
		required[name] = true
	}
	if !knownRequired(t.InputSchema, required) {
		return false
	}
	for name, kind := range allowed {
		p, ok := t.InputSchema.Properties[name]
		if !ok || p.Type != kind {
			return false
		}
	}
	return true
}
func outerSchemas(tools map[string]tool) bool {
	discover, ok := tools[discoverTools]
	query, hasQuery := discover.InputSchema.Properties["query"]
	if !ok || !supportedRoot(discover) || !hasQuery || query.Type != "string" {
		return false
	}
	for _, name := range []string{"limit", "offset"} {
		if p, ok := discover.InputSchema.Properties[name]; ok && p.Type != "integer" {
			return false
		}
	}
	// Optional pagination inputs are generated only when their types are proved.
	if !knownRequired(discover.InputSchema, map[string]bool{"query": true, "limit": true, "offset": true}) {
		return false
	}
	describe, ok := tools[describeTool]
	if !ok || !metaSchema(describe, map[string]string{"operationName": "string"}) {
		return false
	}
	read, ok := tools[callReadTool]
	return ok && metaSchema(read, map[string]string{"operationName": "string", "arguments": "object"})
}
func mergeOperation(tools map[string]tool, resolved map[string]tool) map[string]tool {
	merged := make(map[string]tool, len(tools)+len(resolved))
	for k, v := range tools {
		merged[k] = v
	}
	for k, v := range resolved {
		merged[k] = v
	}
	return merged
}
func (s *session) resolve(ctx context.Context, tools map[string]tool, operation string) (map[string]tool, error) {
	if !allowedLeaf(operation) {
		return nil, ErrUnavailable
	}
	merged := mergeOperation(tools, s.resolved)
	if t, ok := merged[operation]; ok && (!requiresDescription(operation) || t.indirect) {
		return merged, nil
	}
	if !outerSchemas(tools) {
		return nil, s.metaFailure("meta-outer-schema", operation, nil, tools)
	}
	discoverArgs := map[string]any{"query": operation}
	if _, ok := tools[discoverTools].InputSchema.Properties["limit"]; ok {
		discoverArgs["limit"] = 8
	}
	if _, ok := tools[discoverTools].InputSchema.Properties["offset"]; ok {
		discoverArgs["offset"] = 0
	}
	if !verifiedArguments(tools[discoverTools], discoverArgs) {
		return nil, s.metaFailure("meta-outer-schema", operation, nil, tools)
	}
	raw, err := s.invoke(ctx, discoverTools, discoverArgs)
	if err != nil {
		return nil, err
	}
	if !discoveryRead(raw, operation) {
		return nil, s.metaFailure("meta-discover", operation, raw, tools)
	}
	args := map[string]any{"operationName": operation}
	if !verifiedArguments(tools[describeTool], args) {
		return nil, s.metaFailure("meta-outer-schema", operation, nil, tools)
	}
	raw, err = s.invoke(ctx, describeTool, args)
	if err != nil {
		return nil, err
	}
	described, ok := describedRead(raw, operation)
	if !ok {
		return nil, s.metaFailure("meta-describe", operation, raw, tools)
	}
	described.indirect = true
	s.resolved = mergeOperation(s.resolved, map[string]tool{operation: described})
	s.metaTools = tools
	return mergeOperation(tools, s.resolved), nil
}
func (s *session) call(ctx context.Context, name string, args map[string]any) (json.RawMessage, error) {
	if !allowedLeaf(name) {
		return nil, ErrUnavailable
	}
	if described, ok := s.resolved[name]; ok && described.indirect {
		if !verifiedArguments(described, args) {
			return nil, s.metaFailure("meta-inner-arguments", name, nil, s.metaTools)
		}
		if !outerSchemas(s.metaTools) {
			return nil, s.metaFailure("meta-outer-schema", name, nil, s.metaTools)
		}
		if !verifiedArguments(s.metaTools[callReadTool], map[string]any{"operationName": name, "arguments": args}) {
			validation := outerValidationShape(s.metaTools[callReadTool], map[string]any{"operationName": name, "arguments": args})
			return nil, s.metaFailure("meta-outer-arguments", name, nil, s.metaTools, validation)
		}
		raw, err := s.invoke(ctx, callReadTool, map[string]any{"operationName": name, "arguments": args})
		if err != nil {
			return nil, err
		}
		return readPayload(raw), nil
	}
	if requiresDescription(name) {
		return nil, ErrUnavailable
	}
	return s.invoke(ctx, name, args)
}
