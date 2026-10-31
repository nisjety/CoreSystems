package algoliamcp

import (
	"context"
	"encoding/json"
	"sort"
)

type ApplicationProbeOperation struct {
	Name string `json:"name"`
	Read bool   `json:"read"`
}
type ApplicationProbeShape struct {
	Status     string                      `json:"status"`
	Entries    int                         `json:"entries"`
	Operations []ApplicationProbeOperation `json:"operations"`
}

// This is diagnostic discovery only. Its output cannot extend allowedLeaf or
// become an executable operation, application identity or authorization proof.
func (s *session) probeApplications(ctx context.Context, tools map[string]tool) ApplicationProbeShape {
	shape := ApplicationProbeShape{Status: "unsupported", Operations: []ApplicationProbeOperation{}}
	if !outerSchemas(tools) {
		return shape
	}
	args := map[string]any{"query": "applications"}
	for key, value := range map[string]int{"limit": 8, "offset": 0} {
		if _, ok := tools[discoverTools].InputSchema.Properties[key]; ok {
			args[key] = value
		}
	}
	if !verifiedArguments(tools[discoverTools], args) {
		return shape
	}
	raw, err := s.invoke(ctx, discoverTools, args)
	if err != nil {
		shape.Status = "unavailable"
		return shape
	}
	return applicationProbeShape(raw)
}
func applicationProbeShape(raw json.RawMessage) ApplicationProbeShape {
	shape := ApplicationProbeShape{Status: "unsupported", Operations: []ApplicationProbeOperation{}}
	entries, ok := discoveryEntries(raw)
	if !ok {
		return shape
	}
	shape.Status, shape.Entries = "observed", len(entries)
	found := map[string]bool{}
	for _, entry := range entries {
		fields := metadataObject(entry)
		var name string
		for _, key := range []string{"name", "operationName"} {
			if value, ok := fields[key]; ok {
				var candidate string
				if json.Unmarshal(value, &candidate) != nil || (name != "" && name != candidate) {
					name = ""
					break
				}
				name = candidate
			}
		}
		if len(name) > 96 || !diagnosticOperation.MatchString(name) {
			continue
		}
		read := effectRead(fields)
		if previous, exists := found[name]; exists {
			read = read && previous
		}
		found[name] = read
	}
	names := make([]string, 0, len(found))
	for name := range found {
		names = append(names, name)
	}
	sort.Strings(names)
	if len(names) > 3 {
		names = names[:3]
	}
	for _, name := range names {
		shape.Operations = append(shape.Operations, ApplicationProbeOperation{Name: name, Read: found[name]})
	}
	return shape
}
