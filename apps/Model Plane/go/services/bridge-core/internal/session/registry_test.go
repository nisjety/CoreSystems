package session

import (
	"errors"
	"testing"
)

func TestRegistryScopesAndCopiesSessions(t *testing.T) {
	registry := NewRegistry()
	created, err := registry.Register("org-a", "user-a", "web")
	if err != nil {
		t.Fatal(err)
	}
	created.OrgID = "attacker"

	for _, tc := range []struct{ name, org, owner string }{
		{name: "wrong org", org: "org-b", owner: "user-a"},
		{name: "wrong owner", org: "org-a", owner: "user-b"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := registry.GetScoped(created.ID, tc.org, tc.owner); !errors.Is(err, ErrSessionNotFound) {
				t.Fatalf("get error = %v", err)
			}
			if err := registry.UpdateActivityScoped(created.ID, tc.org, tc.owner); !errors.Is(err, ErrSessionNotFound) {
				t.Fatalf("update error = %v", err)
			}
			if err := registry.CloseScoped(created.ID, tc.org, tc.owner); !errors.Is(err, ErrSessionNotFound) {
				t.Fatalf("close error = %v", err)
			}
		})
	}

	visible, err := registry.GetScoped(created.ID, "org-a", "")
	if err != nil {
		t.Fatal(err)
	}
	if visible.OrgID != "org-a" || visible.UserID != "user-a" {
		t.Fatalf("stored session mutated: %#v", visible)
	}
	visible.UserID = "attacker"
	if again, err := registry.GetScoped(created.ID, "org-a", "user-a"); err != nil || again.UserID != "user-a" {
		t.Fatalf("returned session not copied: %#v, %v", again, err)
	}

	if rows := registry.ListScoped("org-a", "user-a"); len(rows) != 1 {
		t.Fatalf("owner list len = %d", len(rows))
	}
	if rows := registry.ListScoped("org-a", "user-b"); len(rows) != 0 {
		t.Fatalf("wrong owner list len = %d", len(rows))
	}
	if err := registry.UpdateActivityScoped(created.ID, "org-a", "user-a"); err != nil {
		t.Fatal(err)
	}
	if err := registry.CloseScoped(created.ID, "org-a", "user-a"); err != nil {
		t.Fatal(err)
	}
	if err := registry.CloseScoped(created.ID, "org-a", "user-a"); !errors.Is(err, ErrSessionClosed) {
		t.Fatalf("second close = %v", err)
	}
	if err := registry.UpdateActivityScoped(created.ID, "org-a", "user-a"); !errors.Is(err, ErrSessionClosed) {
		t.Fatalf("closed update = %v", err)
	}
}

func TestRegisterRejectsInvalidIdentityAndChannel(t *testing.T) {
	registry := NewRegistry()
	for _, tc := range []struct{ org, user, channel string }{
		{user: "user-a", channel: "web"},
		{org: "org-a", channel: "web"},
		{org: "org-a", user: "user-a", channel: "unknown"},
	} {
		if _, err := registry.Register(tc.org, tc.user, tc.channel); err == nil {
			t.Fatalf("register %#v unexpectedly succeeded", tc)
		}
	}
}
