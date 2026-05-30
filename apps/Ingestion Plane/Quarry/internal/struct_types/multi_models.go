package structtypes

type MultiLinkRequestStruct struct {
	URLs     []string `json:"urls"`
	MaxDepth int      `json:"max_depth"`
}
