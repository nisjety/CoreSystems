package authztaxonomy

import "testing"

// WS14-S2: the owner-service registry (one grant writer per resource type).

func TestOwnerServiceIsOnePrincipalPerOwnableType(t *testing.T) {
	service, ok := OwnerService("document")
	if !ok || service != "documents-api" {
		t.Fatalf("OwnerService(document) = %q, %v; want documents-api", service, ok)
	}
	for _, unknown := range []string{"", "nope", "inbox", "Document", "document "} {
		if service, ok := OwnerService(unknown); ok || service != "" {
			t.Errorf("OwnerService(%q) = %q, %v; an unlisted type has no grant writer", unknown, service, ok)
		}
	}
}

func TestEveryOwnerServiceTypeIsOwnableInTheTaxonomy(t *testing.T) {
	// The registry file says a type needs a registry entry AND must already be
	// ownable. A registry entry for a non-ownable type would let a service write
	// grants the taxonomy never asked for.
	ownable := map[string]bool{}
	for _, ty := range Ownable() {
		ownable[ty] = true
	}
	for ty := range ownerServices {
		if !ownable[ty] {
			t.Errorf("owner-service entry %q is not an ownable resource type", ty)
		}
	}
	for ty := range spaceGrantable {
		if !ownable[ty] {
			t.Errorf("space-grantable type %q is not an ownable resource type", ty)
		}
		if _, ok := ownerServices[ty]; !ok {
			t.Errorf("space-grantable type %q has no owner service to write the grant", ty)
		}
	}
}

func TestSpaceGrantableIsDocumentsOnly(t *testing.T) {
	if !SpaceGrantable("document") {
		t.Fatal("a Space may hold a view grant on a document")
	}
	for _, ty := range []string{"", "inbox", "calendar", "nope"} {
		if SpaceGrantable(ty) {
			t.Errorf("SpaceGrantable(%q) must be false", ty)
		}
	}
	if got := len(spaceGrantable); got != 1 {
		t.Errorf("spaceGrantable has %d entries; widening it is a decision, not a drive-by", got)
	}
}

func TestEditIsNotEnforcedForDocuments(t *testing.T) {
	if EditEnforced("document") {
		t.Fatal("document edit is stored but unenforced; Control must keep refusing it")
	}
	if EditEnforced("") || EditEnforced("nope") {
		t.Fatal("an unknown type does not enforce edit")
	}
}
