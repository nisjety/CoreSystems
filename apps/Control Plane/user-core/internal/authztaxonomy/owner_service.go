package authztaxonomy

// Owner-service registry (WS14-S2, plan decision SD-30 / D-1, owner-asserted
// grants).
//
// A resource_grants row is only as trustworthy as the check that the actor may
// administer the resource, and Control cannot see the owning plane's rows.
// Owner-asserted mode therefore lets exactly ONE service per resource type
// write grants for that type, on behalf of a verified delegated user, after it
// has itself proven the actor owns the resource. This registry is that
// one-service-per-type list. It is a Go map (not part of
// resource_taxonomy.json) so the Rust mirror and the taxonomy lint are
// untouched.
//
// A resource type without an entry has no grant writer at all: the facade
// stays fenced for it. Adding a type (a run, a snapshot) needs a registry entry
// here first, and the type must already be ownable in resource_taxonomy.json.

// ownerServices maps an ownable resource type to the one service principal
// allowed to write its grants in owner-asserted mode.
var ownerServices = map[string]string{
	"document": "documents-api",
}

// spaceGrantable lists the resource types a Space may hold a view grant on
// (the owner's attach act on a room file). Documents only.
var spaceGrantable = map[string]struct{}{
	"document": {},
}

// editEnforced lists the resource types whose owning service enforces the
// `edit` role on mutating routes. `edit` is stored but unenforced for
// documents, so a grant at that role would promise access nothing checks:
// Control refuses it (role_not_supported) until the type's owner enforces it.
var editEnforced = map[string]struct{}{}

// OwnerService returns the one service principal that may write grants for the
// resource type in owner-asserted mode, and false when the type has none.
func OwnerService(resourceType string) (string, bool) {
	service, ok := ownerServices[resourceType]
	return service, ok
}

// SpaceGrantable reports whether a Space may be the subject of a grant on the
// resource type.
func SpaceGrantable(resourceType string) bool {
	_, ok := spaceGrantable[resourceType]
	return ok
}

// EditEnforced reports whether the owning service enforces the `edit` role for
// the resource type.
func EditEnforced(resourceType string) bool {
	_, ok := editEnforced[resourceType]
	return ok
}
