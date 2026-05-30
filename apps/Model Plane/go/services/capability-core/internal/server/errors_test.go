package server

import (
	"errors"
	"fmt"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func TestMapErr(t *testing.T) {
	if got := mapErr(nil); got != nil {
		t.Fatalf("expected nil for nil input, got %v", got)
	}
	cases := []struct {
		name string
		in   error
		want codes.Code
	}{
		{"invalid-argument", domain.ErrInvalidArgument, codes.InvalidArgument},
		{"not-found", domain.ErrCapabilityNotFound, codes.NotFound},
		{"version-mismatch", domain.ErrVersionMismatch, codes.FailedPrecondition},
		{"policy-denied", domain.ErrPolicyDenied, codes.PermissionDenied},
		{"policy-denied-wrapped", fmt.Errorf("policy check failed: %w", domain.ErrPolicyDenied), codes.PermissionDenied},
		{"unknown", errors.New("boom"), codes.Internal},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := status.Code(mapErr(tc.in))
			if got != tc.want {
				t.Errorf("mapErr(%v): got %v, want %v", tc.in, got, tc.want)
			}
		})
	}
}
