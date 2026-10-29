package server

import (
	"errors"
	"fmt"
	"testing"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"

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
		{"policy-denied", ErrPolicyDenied, codes.PermissionDenied},
		{"policy-denied-wrapped", fmt.Errorf("policy check failed: %w", ErrPolicyDenied), codes.PermissionDenied},
		{"lease-not-found", lease.ErrLeaseNotFound, codes.NotFound},
		{"lease-expired", lease.ErrLeaseExpired, codes.FailedPrecondition},
		{"snapshot-invalid-lease", snapshot.ErrInvalidLease, codes.FailedPrecondition},
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

func TestTelemetryOutcomeClassifiers(t *testing.T) {
	leaseCases := []struct {
		err  error
		want string
	}{{nil, "ok"}, {ErrPolicyDenied, "policy_denied"}, {lease.ErrLeaseNotFound, "not_found"}, {lease.ErrLeaseExpired, "expired"}, {errors.New("x"), "internal_error"}}
	for _, tc := range leaseCases {
		if got := leaseOutcome(tc.err); got != tc.want {
			t.Fatalf("leaseOutcome(%v)=%q", tc.err, got)
		}
	}
	snapshotLeaseCases := []struct {
		err  error
		want string
	}{{nil, "ok"}, {ErrPolicyDenied, "policy_denied"}, {lease.ErrLeaseNotFound, "lease_not_found"}, {lease.ErrLeaseExpired, "lease_expired"}, {errors.New("x"), "lease_internal_error"}}
	for _, tc := range snapshotLeaseCases {
		if got := snapshotLeaseOutcome(tc.err); got != tc.want {
			t.Fatalf("snapshotLeaseOutcome(%v)=%q", tc.err, got)
		}
	}
	snapshotCreateCases := []struct {
		err  error
		want string
	}{{nil, "ok"}, {ErrPolicyDenied, "policy_denied"}, {snapshot.ErrInvalidLease, "invalid_lease"}, {errors.New("x"), "snapshot_internal_error"}}
	for _, tc := range snapshotCreateCases {
		if got := snapshotCreateOutcome(tc.err); got != tc.want {
			t.Fatalf("snapshotCreateOutcome(%v)=%q", tc.err, got)
		}
	}
}
