package algoliamcp

import "encoding/json"

// ApplicationSchemaShape contains only bounded counts and fixed shape flags for
// the single whitelisted discovery tool. No raw names or enum values escape.
type ApplicationSchemaShape struct {
	ToolPresent         bool   `json:"tool_present"`
	SchemaType          string `json:"schema_type"`
	SchemaOneOf         bool   `json:"schema_one_of"`
	SchemaAnyOf         bool   `json:"schema_any_of"`
	SchemaRef           bool   `json:"schema_ref"`
	PropertiesCount     int    `json:"properties_count"`
	AppPropertyPresent  bool   `json:"app_property_present"`
	AppType             string `json:"app_type"`
	AppOneOf            bool   `json:"app_one_of"`
	AppAnyOf            bool   `json:"app_any_of"`
	AppRef              bool   `json:"app_ref"`
	EnumCount           int    `json:"enum_count"`
	EnumPresent         bool   `json:"enum_present"`
	EnumArray           bool   `json:"enum_array"`
	EnumStrings         bool   `json:"enum_strings"`
	EnumValid           bool   `json:"enum_valid"`
	RequiredCount       int    `json:"required_count"`
	RequiredApplication bool   `json:"required_application"`
	RequiredKnown       bool   `json:"required_known"`
}

func shapeObject(raw json.RawMessage) map[string]json.RawMessage {
	var fields map[string]json.RawMessage
	_ = json.Unmarshal(raw, &fields)
	return fields
}
func present(fields map[string]json.RawMessage, key string) bool { _, ok := fields[key]; return ok }
func boundedShapeCount(n int) int {
	if n > 1024 {
		return 1024
	}
	return n
}
func vettedType(raw json.RawMessage) string {
	if len(raw) == 0 {
		return "missing"
	}
	var value string
	if json.Unmarshal(raw, &value) == nil {
		switch value {
		case "object", "string", "array", "number", "integer", "boolean", "null":
			return value
		}
		return "other"
	}
	var union []json.RawMessage
	if json.Unmarshal(raw, &union) == nil && union != nil {
		return "union"
	}
	return "other"
}
func applicationShape(tools map[string]tool) ApplicationSchemaShape {
	t, found := tools[listIndices]
	shape := ApplicationSchemaShape{ToolPresent: found, SchemaType: "missing", AppType: "missing"}
	if !found {
		return shape
	}
	root := shapeObject(t.RawSchema)
	properties := shapeObject(root["properties"])
	appRaw, appFound := properties["applicationId"]
	app := shapeObject(appRaw)
	shape.SchemaType = vettedType(root["type"])
	shape.SchemaOneOf = present(root, "oneOf")
	shape.SchemaAnyOf = present(root, "anyOf")
	shape.SchemaRef = present(root, "$ref")
	shape.PropertiesCount = boundedShapeCount(len(properties))
	shape.AppPropertyPresent = appFound
	shape.AppType = vettedType(app["type"])
	shape.AppOneOf = present(app, "oneOf")
	shape.AppAnyOf = present(app, "anyOf")
	shape.AppRef = present(app, "$ref")
	var values []json.RawMessage
	shape.EnumPresent = present(app, "enum")
	if json.Unmarshal(app["enum"], &values) == nil && values != nil {
		shape.EnumArray = true
		shape.EnumCount = boundedShapeCount(len(values))
		shape.EnumStrings = len(values) > 0
		shape.EnumValid = len(values) > 0 && len(values) <= 128
		for _, raw := range values {
			var value string
			if json.Unmarshal(raw, &value) != nil {
				shape.EnumStrings = false
				shape.EnumValid = false
			} else if !appPattern.MatchString(value) {
				shape.EnumValid = false
			}
		}
	}
	var required []json.RawMessage
	shape.RequiredKnown = true
	if present(root, "required") && (json.Unmarshal(root["required"], &required) != nil || required == nil) {
		shape.RequiredKnown = false
	}
	shape.RequiredCount = boundedShapeCount(len(required))
	seen := false
	for _, raw := range required {
		var value string
		if json.Unmarshal(raw, &value) != nil || value != "applicationId" || seen {
			shape.RequiredKnown = false
		} else {
			shape.RequiredApplication = true
			seen = true
		}
	}
	return shape
}
