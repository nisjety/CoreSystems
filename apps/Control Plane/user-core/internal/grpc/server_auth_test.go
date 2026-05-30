package grpc

import (
	"context"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

func TestValidateInternalGRPCKey(t *testing.T) {
	t.Parallel()

	const key = "0123456789abcdef0123456789abcdef"
	server := &Server{internalKeys: []string{key}}

	tests := []struct {
		name string
		ctx  context.Context
		code codes.Code
	}{
		{
			name: "valid key",
			ctx:  metadata.NewIncomingContext(context.Background(), metadata.Pairs(internalAPIKeyMetadataKey, key)),
			code: codes.OK,
		},
		{
			name: "missing key",
			ctx:  context.Background(),
			code: codes.Unauthenticated,
		},
		{
			name: "wrong key",
			ctx:  metadata.NewIncomingContext(context.Background(), metadata.Pairs(internalAPIKeyMetadataKey, "wrong-key")),
			code: codes.Unauthenticated,
		},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			err := server.validateInternalGRPCKey(tt.ctx)
			if tt.code == codes.OK {
				if err != nil {
					t.Fatalf("validateInternalGRPCKey() error = %v, want nil", err)
				}
				return
			}
			if got := status.Code(err); got != tt.code {
				t.Fatalf("status.Code(validateInternalGRPCKey()) = %s, want %s", got, tt.code)
			}
		})
	}
}

func TestValidateInternalGRPCKeyRequiresConfiguredSecret(t *testing.T) {
	t.Parallel()

	server := &Server{}
	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs(internalAPIKeyMetadataKey, "anything"))

	if got := status.Code(server.validateInternalGRPCKey(ctx)); got != codes.FailedPrecondition {
		t.Fatalf("status.Code(validateInternalGRPCKey()) = %s, want %s", got, codes.FailedPrecondition)
	}
}
