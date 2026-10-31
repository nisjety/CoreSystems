package bearerrefresh

import "errors"

const defaultCapability = "integration-read"

// capabilityProfile is deployment-owned. Callers cannot supply arbitrary
// audiences, Auth routes, scopes, or audit reasons through the command.
type capabilityProfile struct {
	audience string
	path     string
	scopes   []string
	reason   string
}

func profileFor(capability string) (capabilityProfile, error) {
	switch capability {
	case "", defaultCapability:
		return capabilityProfile{"ingestion", "/api/ingestion/internal-token", []string{"integration:read"}, "Read marketing data for Studio Pulse"}, nil
	case "integration-readwrite":
		// Scope issuance is not permission to skip Integration's separate
		// owner approval and write-attestation checks on each mutation.
		return capabilityProfile{"ingestion", "/api/ingestion/internal-token", []string{"integration:read", "integration:write"}, "Use approved Studio integrations with owner-authorized writes"}, nil
	case "quarry-discovery":
		return capabilityProfile{"quarry", "/api/quarry/internal-token", []string{"map:read"}, "Discover public website URLs for Studio marketing"}, nil
	case "quarry-capture":
		return capabilityProfile{"quarry", "/api/quarry/internal-token", []string{"map:read", "scrape:read", "scrape:write"}, "Capture public website evidence for Studio marketing"}, nil
	case "model-draft":
		return capabilityProfile{"model-gateway", "/api/model-plane/internal-token", []string{"models:invoke"}, "Prepare read-only Studio marketing analysis drafts"}, nil
	case "inference-draft":
		return capabilityProfile{"inference-core", "/api/inference-core/internal-token", []string{"inference:invoke"}, "Prepare read-only Studio marketing inference drafts"}, nil
	default:
		return capabilityProfile{}, errors.New("STUDIO_BEARER_CAPABILITY must select a supported least-privilege profile")
	}
}

// Scope order has no authority meaning; duplicates, missing scopes, and extras
// are rejected. Each profile resolution owns a fresh scope slice.
func exactScopes(actual, expected []string) bool {
	if len(actual) != len(expected) {
		return false
	}
	seen := make(map[string]bool, len(actual))
	for _, scope := range actual {
		if seen[scope] {
			return false
		}
		seen[scope] = true
	}
	for _, scope := range expected {
		if !seen[scope] {
			return false
		}
	}
	return true
}
