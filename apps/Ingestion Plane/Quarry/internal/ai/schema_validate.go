package ai

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/santhosh-tekuri/jsonschema/v6"
)

// ValidateSchema compiles the given JSON Schema string and validates the
// payload against it. Returns nil when valid. The schema is compiled
// against the draft identified by its $schema field (defaults to Draft 2020-12).
//
// This function is safe to call in hot paths — it allocates a fresh compiler
// per invocation (cheap since schemas are typically small extraction schemas).
func ValidateSchema(schemaJSON string, payloadJSON string) error {
	schemaJSON = strings.TrimSpace(schemaJSON)
	if schemaJSON == "" {
		return nil
	}

	var schemaDoc interface{}
	if err := json.Unmarshal([]byte(schemaJSON), &schemaDoc); err != nil {
		return fmt.Errorf("parse schema: %w", err)
	}

	compiler := jsonschema.NewCompiler()
	if err := compiler.AddResource("schema.json", schemaDoc); err != nil {
		return fmt.Errorf("add schema resource: %w", err)
	}

	compiled, err := compiler.Compile("schema.json")
	if err != nil {
		return fmt.Errorf("compile schema: %w", err)
	}

	var payload interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return fmt.Errorf("parse payload: %w", err)
	}

	if err := compiled.Validate(payload); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}
	return nil
}

// IsSchemaValid compiles the schema and returns true when it is syntactically
// valid (no validation errors against the meta-schema). Useful as a pre-check
// before passing a user-supplied schema to extraction endpoints.
func IsSchemaValid(schemaJSON string) bool {
	schemaJSON = strings.TrimSpace(schemaJSON)
	if schemaJSON == "" {
		return false
	}
	var schemaDoc interface{}
	if err := json.Unmarshal([]byte(schemaJSON), &schemaDoc); err != nil {
		return false
	}
	compiler := jsonschema.NewCompiler()
	if err := compiler.AddResource("schema.json", schemaDoc); err != nil {
		return false
	}
	_, err := compiler.Compile("schema.json")
	return err == nil
}
