package ai

import (
	"encoding/json"
	"strings"

	"github.com/rs/zerolog/log"
)

// ModelTier represents the complexity class for model routing.
type ModelTier string

const (
	// TierLight is for simple flat schemas and short text — use fast/cheap model.
	TierLight ModelTier = "light"
	// TierStandard is for moderate schemas — use default model.
	TierStandard ModelTier = "standard"
	// TierHeavy is for deeply nested/recursive schemas or long content — use largest model.
	TierHeavy ModelTier = "heavy"
)

// SchemaComplexity holds the analyzed metrics of a JSON schema.
type SchemaComplexity struct {
	FieldCount  int
	MaxDepth    int
	ArrayCount  int
	ObjectCount int
	HasRef      bool
	HasEnum     bool
	HasOneOf    bool
}

// RouteModel analyzes the extraction schema and HTML size to pick the
// appropriate model tier. This lets ai-core (or a future multi-model
// proxy) dispatch to the right model — saving cost on simple tasks and
// ensuring quality on complex ones.
func RouteModel(schema string, htmlLen int) ModelTier {
	// Dereference $refs before analysis so complexity is measured on the
	// fully-expanded schema, not on a shell that contains only one $ref key.
	if dereffed, err := DereferenceSchema(schema); err == nil {
		schema = dereffed
	}
	complexity := analyzeSchema(schema)

	tier := TierStandard

	switch {
	// Heavy: deeply nested, many fields, recursive refs, or very large HTML.
	case complexity.MaxDepth >= 4 ||
		complexity.FieldCount >= 25 ||
		complexity.HasRef ||
		complexity.HasOneOf ||
		htmlLen > 200_000:
		tier = TierHeavy

	// Light: flat schemas with few fields on short pages.
	case complexity.MaxDepth <= 1 &&
		complexity.FieldCount <= 6 &&
		complexity.ArrayCount == 0 &&
		htmlLen < 50_000:
		tier = TierLight
	}

	log.Debug().
		Str("tier", string(tier)).
		Int("fields", complexity.FieldCount).
		Int("depth", complexity.MaxDepth).
		Int("arrays", complexity.ArrayCount).
		Int("html_len", htmlLen).
		Bool("has_ref", complexity.HasRef).
		Msg("model router selected tier")

	return tier
}

// analyzeSchema parses a JSON schema string and computes complexity metrics.
func analyzeSchema(schema string) SchemaComplexity {
	schema = strings.TrimSpace(schema)
	if schema == "" {
		return SchemaComplexity{}
	}

	var raw map[string]interface{}
	if err := json.Unmarshal([]byte(schema), &raw); err != nil {
		return SchemaComplexity{}
	}

	c := SchemaComplexity{}
	walkSchemaNode(raw, 0, &c)
	return c
}

func walkSchemaNode(node map[string]interface{}, depth int, c *SchemaComplexity) {
	if depth > c.MaxDepth {
		c.MaxDepth = depth
	}

	if _, ok := node["$ref"]; ok {
		c.HasRef = true
	}
	if _, ok := node["enum"]; ok {
		c.HasEnum = true
	}
	if _, ok := node["oneOf"]; ok {
		c.HasOneOf = true
	}
	if _, ok := node["anyOf"]; ok {
		c.HasOneOf = true
	}

	// Count properties.
	if props, ok := node["properties"].(map[string]interface{}); ok {
		c.FieldCount += len(props)
		for _, v := range props {
			if propObj, ok := v.(map[string]interface{}); ok {
				walkSchemaNode(propObj, depth+1, c)
			}
		}
	}

	// Track arrays.
	if typeVal, ok := node["type"].(string); ok {
		switch typeVal {
		case "array":
			c.ArrayCount++
			if items, ok := node["items"].(map[string]interface{}); ok {
				walkSchemaNode(items, depth+1, c)
			}
		case "object":
			c.ObjectCount++
		}
	}
}
