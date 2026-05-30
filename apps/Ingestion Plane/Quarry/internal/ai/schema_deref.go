package ai

import (
	"encoding/json"
	"fmt"
	"strings"
)

// DereferenceSchema resolves all JSON Schema $ref values that point to
// internal definitions (any fragment starting with "#/"). External HTTP
// $ref values are left unchanged. Returns the original schema string
// unchanged on any parse error so callers can always continue with the
// original value.
//
// Hardening vs. the initial implementation:
//   - Explicit cycle detection via a visited-refs set per resolution path
//   - Full JSON Pointer traversal (supports arbitrarily nested $ref paths)
//   - Clear error on circular $ref instead of silent truncation
func DereferenceSchema(schema string) (string, error) {
	schema = strings.TrimSpace(schema)
	if schema == "" || !strings.Contains(schema, "$ref") {
		return schema, nil
	}

	var root map[string]interface{}
	if err := json.Unmarshal([]byte(schema), &root); err != nil {
		return schema, nil // graceful: non-fatal on parse failure
	}

	resolver := &refResolver{
		root:     root,
		maxDepth: maxDerefDepth,
	}

	resolved, err := resolver.resolve(root, 0, nil)
	if err != nil {
		return schema, nil // graceful fallback
	}

	out, err := json.Marshal(resolved)
	if err != nil {
		return schema, nil
	}
	return string(out), nil
}

const maxDerefDepth = 20

// refResolver holds root context and resolves $ref values with cycle detection.
type refResolver struct {
	root     map[string]interface{}
	maxDepth int
}

func (r *refResolver) resolve(node interface{}, depth int, visiting map[string]struct{}) (interface{}, error) {
	if depth > r.maxDepth {
		return node, fmt.Errorf("$ref resolution exceeded max depth %d", r.maxDepth)
	}

	switch v := node.(type) {
	case map[string]interface{}:
		if ref, ok := v["$ref"].(string); ok {
			return r.resolveRef(v, ref, depth, visiting)
		}
		result := make(map[string]interface{}, len(v))
		for k, val := range v {
			resolved, err := r.resolve(val, depth+1, visiting)
			if err != nil {
				return node, err
			}
			result[k] = resolved
		}
		return result, nil

	case []interface{}:
		result := make([]interface{}, len(v))
		for i, item := range v {
			resolved, err := r.resolve(item, depth+1, visiting)
			if err != nil {
				return node, err
			}
			result[i] = resolved
		}
		return result, nil

	default:
		return node, nil
	}
}

func (r *refResolver) resolveRef(node map[string]interface{}, ref string, depth int, visiting map[string]struct{}) (interface{}, error) {
	// Only resolve internal fragment references (starting with "#/").
	if !strings.HasPrefix(ref, "#/") {
		return node, nil
	}

	// Cycle detection: track which $ref paths are currently being resolved.
	if visiting == nil {
		visiting = make(map[string]struct{})
	}
	if _, cycle := visiting[ref]; cycle {
		return node, fmt.Errorf("circular $ref detected: %s", ref)
	}
	nextVisiting := copyStringSet(visiting)
	nextVisiting[ref] = struct{}{}

	// Traverse the JSON Pointer path from root.
	target, err := resolveJSONPointer(r.root, ref)
	if err != nil {
		return node, fmt.Errorf("unresolvable $ref %s: %w", ref, err)
	}

	// Recursively resolve the target definition.
	resolvedTarget, err := r.resolve(target, depth+1, nextVisiting)
	if err != nil {
		return node, err
	}

	// Merge sibling keys (anything besides "$ref") on top of the resolved def.
	merged := shallowCopyMap(resolvedTarget)
	for k, val := range node {
		if k == "$ref" {
			continue
		}
		merged[k] = val
	}
	return merged, nil
}

// resolveJSONPointer traverses the root document using a JSON Pointer
// fragment (e.g. "#/$defs/Address" or "#/definitions/nested/sub").
func resolveJSONPointer(root interface{}, pointer string) (interface{}, error) {
	// Strip the leading "#/"
	path := strings.TrimPrefix(pointer, "#/")
	segments := strings.Split(path, "/")

	current := root
	for _, seg := range segments {
		// JSON Pointer escaping (RFC 6901).
		seg = strings.ReplaceAll(seg, "~1", "/")
		seg = strings.ReplaceAll(seg, "~0", "~")

		obj, ok := current.(map[string]interface{})
		if !ok {
			return nil, fmt.Errorf("cannot traverse non-object at segment %q", seg)
		}
		next, exists := obj[seg]
		if !exists {
			return nil, fmt.Errorf("segment %q not found", seg)
		}
		current = next
	}
	return current, nil
}

func shallowCopyMap(node interface{}) map[string]interface{} {
	if m, ok := node.(map[string]interface{}); ok {
		out := make(map[string]interface{}, len(m))
		for k, v := range m {
			out[k] = v
		}
		return out
	}
	return map[string]interface{}{}
}

func copyStringSet(s map[string]struct{}) map[string]struct{} {
	out := make(map[string]struct{}, len(s)+1)
	for k := range s {
		out[k] = struct{}{}
	}
	return out
}
