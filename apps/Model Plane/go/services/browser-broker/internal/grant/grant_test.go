package grant

import (
	"errors"
	"testing"
	"time"
)

func TestStoreScopesCopiesAndRevokesGrants(t *testing.T) {
	store := NewStore()
	created, err := store.Create(
		"org-a",
		"user-a",
		"session-a",
		"browser://cloud",
		[]string{"example.com"},
		time.Minute,
	)
	if err != nil {
		t.Fatal(err)
	}
	created.OrgID = "attacker"

	for _, tc := range []struct{ name, org, owner string }{
		{name: "wrong org", org: "org-b", owner: "user-a"},
		{name: "wrong owner", org: "org-a", owner: "user-b"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := store.GetScoped(created.ID, tc.org, tc.owner); !errors.Is(err, ErrGrantNotFound) {
				t.Fatalf("error = %v", err)
			}
			if err := store.RevokeScoped(created.ID, tc.org, tc.owner); !errors.Is(err, ErrGrantNotFound) {
				t.Fatalf("revoke error = %v", err)
			}
		})
	}

	visible, err := store.GetScoped(created.ID, "org-a", "")
	if err != nil {
		t.Fatal(err)
	}
	if visible.OrgID != "org-a" || visible.OwnerID != "user-a" {
		t.Fatalf("stored grant mutated: %#v", visible)
	}
	visible.OwnerID = "attacker"
	visibleAgain, err := store.GetScoped(created.ID, "org-a", "user-a")
	if err != nil || visibleAgain.OwnerID != "user-a" {
		t.Fatalf("returned grant was not copied: %#v, %v", visibleAgain, err)
	}

	if err := store.RevokeScoped(created.ID, "org-a", "user-a"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(created.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantRevoked) {
		t.Fatalf("error = %v, want revoked", err)
	}
}

func TestStoreReportsExpiredAndUnknownGrants(t *testing.T) {
	store := NewStore()
	expired, err := store.Create(
		"org-a",
		"user-a",
		"session-a",
		"browser://cloud",
		[]string{"example.com"},
		-time.Second,
	)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(expired.ID, "org-a", "user-a"); !errors.Is(err, ErrGrantExpired) {
		t.Fatalf("error = %v, want expired", err)
	}
	if _, err := store.GetScoped("missing", "org-a", "user-a"); !errors.Is(err, ErrGrantNotFound) {
		t.Fatalf("error = %v", err)
	}
}

func TestNormalizeAllowedDomainsRejectsUnboundedOrNonHostPolicy(t *testing.T) {
	for _, domains := range [][]string{
		nil,
		{},
		{""},
		{"https://example.com"},
		{"example.com/path"},
		{"*.example.com"},
		{"127.0.0.1"},
		{"localhost"},
	} {
		if _, err := NormalizeAllowedDomains(domains); !errors.Is(err, ErrInvalidDomainPolicy) {
			t.Fatalf("NormalizeAllowedDomains(%#v) error = %v, want ErrInvalidDomainPolicy", domains, err)
		}
	}
}

func TestStoreCopiesCanonicalAllowedDomains(t *testing.T) {
	store := NewStore()
	created, err := store.Create(
		"org-a",
		"user-a",
		"session-a",
		"browser://cloud",
		[]string{"EXAMPLE.com", "api.example.com", "example.com"},
		time.Minute,
	)
	if err != nil {
		t.Fatal(err)
	}
	if got, want := created.AllowedDomains, []string{"api.example.com", "example.com"}; !sameStrings(got, want) {
		t.Fatalf("created policy = %#v, want %#v", got, want)
	}
	created.AllowedDomains[0] = "attacker.example"
	stored, err := store.GetScoped(created.ID, "org-a", "user-a")
	if err != nil {
		t.Fatal(err)
	}
	if got, want := stored.AllowedDomains, []string{"api.example.com", "example.com"}; !sameStrings(got, want) {
		t.Fatalf("stored policy = %#v, want %#v", got, want)
	}
}

func sameStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for index := range got {
		if got[index] != want[index] {
			return false
		}
	}
	return true
}
