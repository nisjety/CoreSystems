package algoliamcp

import (
	"bytes"
	"encoding/json"
	"strings"
)

type ResourcePathShape struct {
	Path             string `json:"path"`
	Kind             string `json:"kind"`
	Count            int    `json:"count"`
	CountCapped      bool   `json:"count_capped"`
	IdentifiersValid bool   `json:"identifiers_valid"`
}
type ResourceShape struct {
	KnownRootKeys      []string            `json:"known_root_keys"`
	Paths              []ResourcePathShape `json:"paths"`
	AppDefaultPresent  bool                `json:"app_default_present"`
	AppExamplesPresent bool                `json:"app_examples_present"`
}

func dataKind(raw json.RawMessage) string {
	raw = bytes.TrimSpace(raw)
	if len(raw) == 0 {
		return "missing"
	}
	switch raw[0] {
	case '[':
		return "array"
	case '{':
		return "object"
	case '"':
		return "string"
	case 'n':
		return "null"
	case 't', 'f':
		return "boolean"
	}
	return "number"
}
func resourceShape(raw json.RawMessage) ResourceShape {
	shape := ResourceShape{KnownRootKeys: []string{}, Paths: []ResourcePathShape{}}
	root := shapeObject(raw)
	operation := metadataObject(raw)
	for _, key := range []string{"applications", "availableApplications", "allowedApplications", "applicationIds", "context", "metadata", "resources", "operation", "tool", "result", "data", "operationName", "name", "effect", "inputSchema", "schema", "parameters"} {
		if present(root, key) {
			shape.KnownRootKeys = append(shape.KnownRootKeys, key)
		}
	}
	locations := []struct {
		name   string
		fields map[string]json.RawMessage
	}{{"root", root}, {"operation", operation}, {"context", shapeObject(root["context"])}, {"metadata", shapeObject(root["metadata"])}, {"resources", shapeObject(root["resources"])}, {"operation.context", shapeObject(operation["context"])}, {"operation.metadata", shapeObject(operation["metadata"])}, {"operation.resources", shapeObject(operation["resources"])}}
	// root==operation would otherwise duplicate a direct metadata path.
	direct := present(root, "operationName") || present(root, "name") || !(present(root, "operation") || present(root, "tool") || present(root, "result") || present(root, "data"))
	for _, location := range locations {
		if strings.HasPrefix(location.name, "operation") && direct {
			continue
		}
		for _, key := range []string{"applications", "availableApplications", "allowedApplications", "applicationIds"} {
			value, ok := location.fields[key]
			if !ok {
				continue
			}
			entry := ResourcePathShape{Path: location.name + "." + key, Kind: dataKind(value)}
			var items []json.RawMessage
			if json.Unmarshal(value, &items) == nil && items != nil {
				entry.Count = len(items)
				entry.IdentifiersValid = len(items) > 0 && len(items) <= 128
				if len(items) > 128 {
					entry.Count = 128
					entry.CountCapped = true
				} else {
					for _, item := range items {
						var id string
						if json.Unmarshal(item, &id) != nil {
							fields := shapeObject(item)
							if json.Unmarshal(fields["id"], &id) != nil {
								_ = json.Unmarshal(fields["applicationId"], &id)
							}
						}
						if !appPattern.MatchString(id) {
							entry.IdentifiersValid = false
						}
					}
				}
			}
			shape.Paths = append(shape.Paths, entry)
		}
	}
	for _, key := range []string{"inputSchema", "schema", "parameters"} {
		if candidate, ok := operation[key]; ok {
			app := shapeObject(shapeObject(shapeObject(candidate)["properties"])["applicationId"])
			shape.AppDefaultPresent = present(app, "default")
			shape.AppExamplesPresent = present(app, "examples")
			break
		}
	}
	return shape
}
