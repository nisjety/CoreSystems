package algoliamcp

import "encoding/json"

type ReportInputShape struct {
	Type            string              `json:"type"`
	RootSupported   bool                `json:"root_supported"`
	PropertiesCount int                 `json:"properties_count"`
	RequiredCount   int                 `json:"required_count"`
	RequiredKnown   bool                `json:"required_known"`
	Inputs          []MetaArgumentShape `json:"inputs"`
}
type ReportFieldShape struct {
	Name string `json:"name"`
	Kind string `json:"kind"`
}
type ReportArrayShape struct {
	Path             string             `json:"path"`
	Count            int                `json:"count"`
	CountCapped      bool               `json:"count_capped"`
	FirstRowKeyCount int                `json:"first_row_key_count"`
	Fields           []ReportFieldShape `json:"fields"`
}
type ReportResultShape struct {
	Kind         string             `json:"kind"`
	RootKeyCount int                `json:"root_key_count"`
	KnownKeys    []string           `json:"known_keys"`
	Arrays       []ReportArrayShape `json:"arrays"`
}

func reportInputShape(t tool) ReportInputShape {
	shape := ReportInputShape{Type: vettedType(shapeObject(t.RawSchema)["type"]), RootSupported: supportedRoot(t), PropertiesCount: cappedSchemaCount(len(t.InputSchema.Properties)), RequiredCount: cappedSchemaCount(len(t.InputSchema.Required)), RequiredKnown: knownRequired(t.InputSchema, map[string]bool{"applicationId": true, "indexName": true, "index": true, "startDate": true, "endDate": true, "limit": true, "direction": true, "orderBy": true}), Inputs: []MetaArgumentShape{}}
	props := shapeObject(shapeObject(t.RawSchema)["properties"])
	for _, name := range []string{"applicationId", "indexName", "index", "startDate", "endDate", "limit", "size", "offset", "direction", "orderBy", "clickAnalytics", "revenueAnalytics", "originalQuery", "userIntent"} {
		raw, exists := props[name]
		shape.Inputs = append(shape.Inputs, MetaArgumentShape{Name: name, Present: exists, Type: vettedType(shapeObject(raw)["type"])})
	}
	return shape
}
func reportResultShape(raw json.RawMessage) ReportResultShape {
	root := shapeObject(raw)
	shape := ReportResultShape{Kind: dataKind(raw), RootKeyCount: cappedSchemaCount(len(root)), KnownKeys: []string{}, Arrays: []ReportArrayShape{}}
	for _, name := range []string{"searches", "results", "data", "items", "rows", "count", "noResultCount", "rate", "dates", "error", "errors", "isError", "nextCursor", "hasMore"} {
		if present(root, name) {
			shape.KnownKeys = append(shape.KnownKeys, name)
		}
	}
	locations := []struct {
		path string
		raw  json.RawMessage
	}{{"root", raw}, {"searches", root["searches"]}, {"results", root["results"]}, {"data", root["data"]}, {"items", root["items"]}, {"rows", root["rows"]}}
	for _, location := range locations {
		var rows []json.RawMessage
		if json.Unmarshal(location.raw, &rows) != nil || rows == nil {
			continue
		}
		array := ReportArrayShape{Path: location.path, Count: cappedSchemaCount(len(rows)), CountCapped: len(rows) > 128, Fields: []ReportFieldShape{}}
		if len(rows) > 0 {
			row := shapeObject(rows[0])
			array.FirstRowKeyCount = cappedSchemaCount(len(row))
			for _, name := range []string{"search", "query", "count", "nbHits", "averageResults", "noResultCount", "date"} {
				if value, exists := row[name]; exists {
					array.Fields = append(array.Fields, ReportFieldShape{Name: name, Kind: dataKind(value)})
				}
			}
		}
		shape.Arrays = append(shape.Arrays, array)
	}
	return shape
}
