package algoliamcp

import (
	"bytes"
	"context"
	"encoding/json"
)

const applicationList = "algolia_application_list"

type ApplicationListArrayShape struct {
	Path                string `json:"path"`
	Count               int    `json:"count"`
	CountCapped         bool   `json:"count_capped"`
	IDString            bool   `json:"id_string"`
	ApplicationIDString bool   `json:"application_id_string"`
}
type ApplicationListProbeShape struct {
	SchemaType          string                      `json:"schema_type"`
	PropertiesCount     int                         `json:"properties_count"`
	RequiredCount       int                         `json:"required_count"`
	EmptyArgumentsValid bool                        `json:"empty_arguments_valid"`
	Status              string                      `json:"status"`
	ResultKind          string                      `json:"result_kind"`
	KnownResultKeys     []string                    `json:"known_result_keys"`
	Arrays              []ApplicationListArrayShape `json:"arrays"`
}

// The hosted provider exposed this exact read-only operation via discovery.
// This one-shot probe establishes its machine contract without interpreting
// descriptions, enabling arbitrary operations or returning guessed resources.
func (s *session) hostedApplications(ctx context.Context, tools map[string]tool) (items []Application, resultErr error) {
	resultErr = ErrUnavailable
	shape := ApplicationListProbeShape{Status: "unsupported", KnownResultKeys: []string{}, Arrays: []ApplicationListArrayShape{}}
	defer func() {
		if resultErr != nil && s.diagnostic != nil {
			s.diagnostic(Diagnostic{Phase: "application-list-probe", HTTPStatus: 200, ApplicationListProbe: &shape})
		}
	}()
	resolved, err := s.resolve(ctx, tools, applicationList)
	if err != nil {
		return nil, err
	}
	t := resolved[applicationList]
	shape.SchemaType = vettedType(shapeObject(t.RawSchema)["type"])
	shape.PropertiesCount = cappedSchemaCount(len(t.InputSchema.Properties))
	shape.RequiredCount = cappedSchemaCount(len(t.InputSchema.Required))
	shape.EmptyArgumentsValid = verifiedEmptyArguments(t)
	if !shape.EmptyArgumentsValid {
		return nil, ErrUnavailable
	}
	raw, err := s.call(ctx, applicationList, map[string]any{})
	if err != nil {
		shape.Status = "unavailable"
		return nil, err
	}
	shape.Status = "observed"
	shape.ResultKind = dataKind(raw)
	fields := shapeObject(raw)
	for _, key := range []string{"applications", "items", "data", "results", "nextCursor", "hasMore", "total", "count"} {
		if present(fields, key) {
			shape.KnownResultKeys = append(shape.KnownResultKeys, key)
		}
	}
	locations := []struct {
		path string
		raw  json.RawMessage
	}{{"root", raw}, {"applications", fields["applications"]}, {"items", fields["items"]}, {"data", fields["data"]}, {"results", fields["results"]}}
	for _, location := range locations {
		var entries []json.RawMessage
		if json.Unmarshal(location.raw, &entries) != nil || entries == nil {
			continue
		}
		array := ApplicationListArrayShape{Path: location.path, Count: cappedSchemaCount(len(entries)), CountCapped: len(entries) > 128}
		if len(entries) > 0 {
			first := shapeObject(entries[0])
			var id string
			array.IDString = json.Unmarshal(first["id"], &id) == nil && dataKind(first["id"]) == "string"
			array.ApplicationIDString = json.Unmarshal(first["applicationId"], &id) == nil && dataKind(first["applicationId"]) == "string"
		}
		shape.Arrays = append(shape.Arrays, array)
	}
	return parseHostedApplications(raw)
}

func verifiedEmptyArguments(t tool) bool {
	if !verifiedArguments(t, map[string]any{}) {
		return false
	}
	root := shapeObject(t.RawSchema)
	if raw, ok := root["properties"]; ok && shapeObject(raw) == nil {
		return false
	}
	if raw, ok := root["required"]; ok {
		var required []string
		if json.Unmarshal(raw, &required) != nil || required == nil || len(required) != 0 {
			return false
		}
	}
	if raw, ok := root["additionalProperties"]; ok {
		value := bytes.TrimSpace(raw)
		if bytes.Equal(value, []byte("true")) || bytes.Equal(value, []byte("false")) {
			return true
		}
		object := shapeObject(raw)
		return object != nil && len(object) == 0
	}
	return true
}
