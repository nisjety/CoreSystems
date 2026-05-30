package policy_test

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

func TestEngine_Enforce_RBAC(t *testing.T) {
	reg := registry.NewRegistry()

	roleCaps := map[string][]string{
		"reader":  {"cap.memory.search", "cap.retrieval.query"},
		"writer":  {"cap.memory.index"},
		"browser": {"cap.browser.open"},
	}
	subjectRoles := map[string][]string{
		"alice":   {"reader"},
		"bob":     {"reader", "writer"},
		"charlie": {"browser"},
	}

	eng := policy.NewWithRBAC(reg, subjectRoles, roleCaps)

	t.Run("unknown subject is denied", func(t *testing.T) {
		err := eng.Enforce(context.Background(), "mallory", "cap.memory.search")
		if !errors.Is(err, domain.ErrPermissionDenied) {
			t.Fatalf("expected ErrPermissionDenied, got %v", err)
		}
	})

	t.Run("subject with granting role is allowed", func(t *testing.T) {
		if err := eng.Enforce(context.Background(), "alice", "cap.memory.search"); err != nil {
			t.Fatalf("expected nil, got %v", err)
		}
		if err := eng.Enforce(context.Background(), "bob", "cap.memory.index"); err != nil {
			t.Fatalf("expected nil, got %v", err)
		}
	})

	t.Run("subject without granting role is denied", func(t *testing.T) {
		err := eng.Enforce(context.Background(), "alice", "cap.memory.index")
		if !errors.Is(err, domain.ErrPermissionDenied) {
			t.Fatalf("expected ErrPermissionDenied, got %v", err)
		}
	})

	t.Run("empty subject or capability is invalid argument", func(t *testing.T) {
		if err := eng.Enforce(context.Background(), "", "cap.memory.search"); !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument for empty subject, got %v", err)
		}
		if err := eng.Enforce(context.Background(), "alice", ""); !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument for empty cap, got %v", err)
		}
	})

	t.Run("unknown capability surfaces registry error", func(t *testing.T) {
		err := eng.Enforce(context.Background(), "alice", "cap.does.not.exist")
		if err == nil {
			t.Fatalf("expected error for unknown capability, got nil")
		}
		if errors.Is(err, domain.ErrPermissionDenied) {
			t.Fatalf("unknown capability should not short-circuit to permission denied: %v", err)
		}
	})
}
