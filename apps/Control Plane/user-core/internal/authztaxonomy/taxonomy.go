// Package authztaxonomy is the single source of truth for which resource types
// are user-ownable versus team-shared in the Per-User Data Ownership & Sharing
// model.
//
//   - OWNABLE types (e.g. document) may be private to their creator and support
//     per-user grants via resource_grants + a visibility column.
//   - TEAM_SHARED types (inbox/conversation/ticket, billing, audit_log,
//     org_settings, quarry_source/run, capability_registry) are always
//     org-scoped. Private visibility and per-user grants are REJECTED for them.
//
// The canonical lists live in resource_taxonomy.json (embedded below). The
// retrieval-engine-rs Rust mirror and scripts/lint-resource-taxonomy.sh assert
// cross-language parity against that same file, so a type can never be ownable
// in one plane and team-shared in another.
package authztaxonomy

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"sort"
)

//go:embed resource_taxonomy.json
var taxonomyJSON []byte

// Category classifies a resource type.
type Category string

const (
	CategoryOwnable    Category = "ownable"
	CategoryTeamShared Category = "team_shared"
)

var (
	ownable    = map[string]struct{}{}
	teamShared = map[string]struct{}{}
)

func init() {
	var doc struct {
		Ownable    []string `json:"ownable"`
		TeamShared []string `json:"team_shared"`
	}
	if err := json.Unmarshal(taxonomyJSON, &doc); err != nil {
		// The taxonomy is embedded at build time; malformed JSON is a build-time
		// authoring error and must fail loudly rather than silently disabling the
		// privacy guard.
		panic(fmt.Sprintf("authztaxonomy: invalid resource_taxonomy.json: %v", err))
	}
	for _, t := range doc.Ownable {
		ownable[t] = struct{}{}
	}
	for _, t := range doc.TeamShared {
		teamShared[t] = struct{}{}
	}
}

// IsOwnable reports whether the resource type may be user-owned/private.
func IsOwnable(resourceType string) bool {
	_, ok := ownable[resourceType]
	return ok
}

// IsTeamShared reports whether the resource type is always org-scoped.
func IsTeamShared(resourceType string) bool {
	_, ok := teamShared[resourceType]
	return ok
}

// Classify returns the category of a resource type. It errors if the type is
// unknown (not classified) or — a taxonomy bug — present in both sets.
func Classify(resourceType string) (Category, error) {
	o, s := IsOwnable(resourceType), IsTeamShared(resourceType)
	switch {
	case o && s:
		return "", fmt.Errorf("resource type %q is classified as BOTH ownable and team-shared (taxonomy bug)", resourceType)
	case o:
		return CategoryOwnable, nil
	case s:
		return CategoryTeamShared, nil
	default:
		return "", fmt.Errorf("resource type %q is not classified in the ownership taxonomy", resourceType)
	}
}

// ValidateUserGrant returns nil only if a per-user / private-visibility grant is
// permitted for the resource type. Unknown and team-shared types are rejected —
// this is the guard that stops a "private inbox" or "share a billing record with
// one user" from ever being created.
func ValidateUserGrant(resourceType string) error {
	cat, err := Classify(resourceType)
	if err != nil {
		return err
	}
	if cat != CategoryOwnable {
		return fmt.Errorf("resource type %q is team-shared; per-user grants and private visibility are not allowed", resourceType)
	}
	return nil
}

// Ownable returns the sorted list of ownable resource types.
func Ownable() []string { return sortedKeys(ownable) }

// TeamShared returns the sorted list of team-shared resource types.
func TeamShared() []string { return sortedKeys(teamShared) }

func sortedKeys(m map[string]struct{}) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}
