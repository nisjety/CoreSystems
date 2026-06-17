package insights

const (
	ServicePlane = "application-plane"
)

func OwnershipDecision() PlanePlacement {
	return PlanePlacement{
		ServicePlane:     ServicePlane,
		ControlPlaneRole: "authn/authz, organization membership, quotas, entitlements, and audit only",
		Rules: []string{
			"insight-core owns Velion workspace insight projections, not identity or billing authority",
			"org scope is mandatory on every read and ingest boundary",
			"external analytics credentials are leased through integration-core and are never stored inline",
			"cross-plane data arrives through APIs or events; no direct Control, Data, Model, or Ingestion database access",
		},
	}
}
