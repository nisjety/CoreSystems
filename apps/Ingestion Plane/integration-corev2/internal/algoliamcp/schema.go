package algoliamcp

import (
	"context"
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"sort"
)

const listIndices = "algolia_search_list_indices"
const noResults = "algolia_analytics_no_results_rate"
const searchCount = "algolia_analytics_number_of_searches"

type property struct {
	Type string   `json:"type"`
	Enum []string `json:"enum"`
}
type schema struct {
	Type       string              `json:"type"`
	Properties map[string]property `json:"properties"`
	Required   []string            `json:"required"`
}
type tool struct {
	Name             string          `json:"name"`
	InputSchema      schema          `json:"inputSchema"`
	RawSchema        json.RawMessage `json:"-"`
	indirect         bool
	ResourceMetadata json.RawMessage `json:"-"`
}

func (s *session) tools(ctx context.Context) (map[string]tool, error) {
	raw, err := s.rpc(ctx, "tools/list", map[string]any{})
	if err != nil {
		return nil, err
	}
	var result struct {
		Tools      []json.RawMessage `json:"tools"`
		NextCursor string            `json:"nextCursor"`
	}
	if json.Unmarshal(raw, &result) != nil || len(result.Tools) > 256 || len(result.Tools) == 0 || result.NextCursor != "" {
		return nil, s.failure("tools/list", 200, nil)
	}
	found := map[string]tool{}
	names := make([]string, 0, len(result.Tools))
	for _, item := range result.Tools {
		var name struct {
			Name string `json:"name"`
		}
		if json.Unmarshal(item, &name) != nil {
			return nil, s.failure("tools/list", 200, nil)
		}
		names = append(names, name.Name)
		if !allowedLeaf(name.Name) && !allowedMeta(name.Name) {
			continue
		}
		if _, exists := found[name.Name]; exists {
			return nil, s.failure("tools/list", 200, nil)
		}
		var value tool
		if json.Unmarshal(item, &value) != nil || value.InputSchema.Type != "object" {
			return nil, s.failure("tools/list", 200, nil)
		}
		var fields map[string]json.RawMessage
		_ = json.Unmarshal(item, &fields)
		value.RawSchema = fields["inputSchema"]
		found[name.Name] = value
	}
	s.catalog = documentedCatalog(names)
	return found, nil
}
func applications(tools map[string]tool) ([]Application, error) {
	t, ok := tools[listIndices]
	if !ok {
		return nil, ErrUnavailable
	}
	p, ok := t.InputSchema.Properties["applicationId"]
	if !ok || p.Type != "string" || len(p.Enum) == 0 || len(p.Enum) > 128 || !supportedRoot(t) {
		return nil, ErrUnavailable
	}
	if !knownRequired(t.InputSchema, map[string]bool{"applicationId": true}) {
		return nil, ErrUnavailable
	}
	apps := []Application{}
	seen := map[string]bool{}
	for _, id := range p.Enum {
		if !appPattern.MatchString(id) || seen[id] || !verifiedArguments(t, map[string]any{"applicationId": id}) {
			return nil, ErrUnavailable
		}
		seen[id] = true
		apps = append(apps, Application{ID: id})
	}
	sort.Slice(apps, func(i, j int) bool { return apps[i].ID < apps[j].ID })
	return apps, nil
}
func knownRequired(s schema, allowed map[string]bool) bool {
	seen := map[string]bool{}
	for _, name := range s.Required {
		if !allowed[name] || seen[name] {
			return false
		}
		seen[name] = true
	}
	return true
}
func (s *session) invoke(ctx context.Context, name string, args map[string]any) (json.RawMessage, error) {
	if !allowedLeaf(name) && !allowedMeta(name) {
		return nil, ErrUnavailable
	}
	raw, err := s.rpc(ctx, "tools/call", map[string]any{"name": name, "arguments": args})
	if err != nil {
		return nil, err
	}
	var result struct {
		IsError    bool            `json:"isError"`
		Structured json.RawMessage `json:"structuredContent"`
		Content    []struct {
			Type string `json:"type"`
			Text string `json:"text"`
		} `json:"content"`
	}
	if json.Unmarshal(raw, &result) != nil || result.IsError {
		return nil, ErrUnavailable
	}
	var data json.RawMessage
	if len(result.Structured) > 0 && string(result.Structured) != "null" {
		data = result.Structured
	} else if len(result.Content) == 1 && result.Content[0].Type == "text" {
		data = json.RawMessage(result.Content[0].Text)
	} else {
		return nil, ErrUnavailable
	}
	if len(data) > maxResponse || webvitals.UniqueJSON(data) != nil {
		return nil, ErrUnavailable
	}
	return data, nil
}
func (s *session) indices(ctx context.Context, tools map[string]tool, app string) ([]Index, error) {
	var err error
	tools, err = s.resolve(ctx, tools, listIndices)
	if err != nil {
		return nil, err
	}
	t := tools[listIndices]
	args := map[string]any{"applicationId": app}
	if !appPattern.MatchString(app) || !knownRequired(t.InputSchema, map[string]bool{"applicationId": true}) || !verifiedArguments(t, args) {
		return nil, ErrAccess
	}
	// An explicitly requested ID is not discovery or proof of authorization.
	// If the provider supplies an enum it must restrict the request; otherwise
	// only the authenticated, schema-validated list response below proves access.
	appProperty := shapeObject(shapeObject(shapeObject(t.RawSchema)["properties"])["applicationId"])
	if present(appProperty, "enum") {
		apps, err := applications(tools)
		if err != nil {
			return nil, err
		}
		allowed := false
		for _, a := range apps {
			allowed = allowed || a.ID == app
		}
		if !allowed {
			return nil, ErrAccess
		}
	}
	raw, err := s.call(ctx, listIndices, args)
	if err != nil {
		return nil, err
	}
	var result struct {
		Items []struct {
			Name string `json:"name"`
		} `json:"items"`
	}
	if json.Unmarshal(raw, &result) != nil || result.Items == nil || len(result.Items) > 5000 {
		if s.resolved[listIndices].indirect {
			return nil, s.metaFailure("meta-read-result", listIndices, raw, tools)
		}
		return nil, ErrUnavailable
	}
	out := []Index{}
	seen := map[string]bool{}
	for _, item := range result.Items {
		if !indexPattern.MatchString(item.Name) || seen[item.Name] {
			return nil, ErrUnavailable
		}
		seen[item.Name] = true
		out = append(out, Index{Name: item.Name})
	}
	return out, nil
}
func analyticsArgs(t tool, app, index string, w sitesources.Window) (map[string]any, error) {
	if t.InputSchema.Type != "object" {
		return nil, ErrUnavailable
	}
	props := t.InputSchema.Properties
	indexKey := "indexName"
	_, named := props[indexKey]
	_, alternate := props["index"]
	if named == alternate {
		return nil, ErrUnavailable
	}
	if alternate {
		indexKey = "index"
	}
	allowed := map[string]bool{"applicationId": true, indexKey: true, "startDate": true, "endDate": true}
	if !knownRequired(t.InputSchema, allowed) {
		return nil, ErrUnavailable
	}
	args := map[string]any{"applicationId": app, indexKey: index, "startDate": w.StartDate, "endDate": w.EndDate}
	for key, value := range args {
		p, ok := props[key]
		if !ok || p.Type != "string" {
			return nil, ErrUnavailable
		}
		if len(p.Enum) > 0 {
			matches := false
			for _, v := range p.Enum {
				matches = matches || v == value
			}
			if !matches {
				return nil, ErrUnavailable
			}
		}
	}
	if !verifiedArguments(t, args) {
		return nil, ErrUnavailable
	}
	return args, nil
}
