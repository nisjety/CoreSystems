package server

import (
	"errors"
	"fmt"
	"testing"

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
		{"invalid-argument", ErrInvalidArgument, codes.InvalidArgument},
		{"invalid-argument-wrapped", fmt.Errorf("bad input: %w", ErrInvalidArgument), codes.InvalidArgument},
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
