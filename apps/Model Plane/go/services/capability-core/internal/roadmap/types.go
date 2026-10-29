package roadmap

// Feature describes a single roadmap/gap catalog entry. The Status field is
// constrained to the values "yes", "partial", or "no" and is validated by the
// catalog test-suite rather than the Go type system so the data can be loaded
// from static literals without a builder layer.
type Feature struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Category    string `json:"category,omitempty"`
	Status      string `json:"status"`
	Description string `json:"description,omitempty"`
	Owner       string `json:"owner,omitempty"`
}

// Catalog is the top-level implementation-status document. It separates
// per-service coverage from cross-cutting backend/product-shell concerns and
// donor-feature roadmaps so callers can render each section independently.
type Catalog struct {
	ServiceChecklist   []Feature `json:"serviceChecklist"`
	BackendRuntime     []Feature `json:"backendRuntime"`
	ProductShell       []Feature `json:"productShell"`
	ClaudeDonorRoadmap []Feature `json:"claudeDonorRoadmap"`
	ModelPlaneV2Parity []Feature `json:"modelPlaneV2Parity"`
}

// Load returns the current, in-code implementation-status catalog. The data is
// deliberately static — it is the source of truth that docs/gap-analysis.md
// mirrors.
func Load() Catalog {
	return catalog
}
