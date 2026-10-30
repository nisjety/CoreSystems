package algoliamcp

// Diagnostic is deliberately content-free. It cannot carry upstream messages,
// URLs, arguments, schemas, response bodies, identities or credential material.
type Diagnostic struct {
	Phase                string                     `json:"phase"`
	HTTPStatus           int                        `json:"httpStatus,omitempty"`
	RPCCode              *int                       `json:"rpcCode,omitempty"`
	SchemaShape          *ApplicationSchemaShape    `json:"schemaShape,omitempty"`
	CatalogShape         *ToolCatalogShape          `json:"catalogShape,omitempty"`
	MetaShape            *MetaShape                 `json:"metaShape,omitempty"`
	ResourceShape        *ResourceShape             `json:"resourceShape,omitempty"`
	ApplicationProbe     *ApplicationProbeShape     `json:"applicationProbe,omitempty"`
	ApplicationListProbe *ApplicationListProbeShape `json:"applicationListProbe,omitempty"`
}

func phase(method string) string {
	switch method {
	case "initialize":
		return "initialize"
	case "notifications/initialized":
		return "initialized"
	case "tools/list":
		return "tools-list"
	case "tools/call":
		return "tool-call"
	}
	return ""
}
func (s *session) failure(method string, status int, code *int) error {
	if s.diagnostic != nil && phase(method) != "" {
		s.diagnostic(Diagnostic{Phase: phase(method), HTTPStatus: status, RPCCode: code})
	}
	return ErrUnavailable
}
func (s *Service) applicationSchemaFailure(tools map[string]tool, catalog ToolCatalogShape, probe *ApplicationProbeShape) {
	if s.diagnostic != nil {
		shape := applicationShape(tools)
		resources := resourceShape(tools[listIndices].ResourceMetadata)
		s.diagnostic(Diagnostic{Phase: "application-schema", HTTPStatus: 200, SchemaShape: &shape, CatalogShape: &catalog, ResourceShape: &resources, ApplicationProbe: probe})
	}
}
