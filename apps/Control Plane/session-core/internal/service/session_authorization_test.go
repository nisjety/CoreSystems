package service

import (
	"errors"
	"testing"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/domain"
)

func TestRequireSessionOwnerPinsAccessToVerifiedSubject(t *testing.T) {
	session := &domain.Session{ID: "session-a", UserID: "user-owner"}

	if err := requireSessionOwner(session, "user-owner"); err != nil {
		t.Fatalf("owner rejected: %v", err)
	}
	for _, actor := range []string{"", "user-attacker"} {
		if err := requireSessionOwner(session, actor); !errors.Is(err, domain.ErrSessionAccessDenied) {
			t.Fatalf("actor %q error = %v; want ErrSessionAccessDenied", actor, err)
		}
	}
}

func TestRequireSessionOwnerRejectsMissingAggregate(t *testing.T) {
	if err := requireSessionOwner(nil, "user-owner"); !errors.Is(err, domain.ErrSessionAccessDenied) {
		t.Fatalf("nil session error = %v; want ErrSessionAccessDenied", err)
	}
}
