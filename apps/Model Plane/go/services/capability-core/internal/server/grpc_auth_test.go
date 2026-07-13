package server

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

type grpcAuthClaims struct {
	OrgID         string   `json:"org_id"`
	ServiceID     string   `json:"service_id"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes"`
	jwt.RegisteredClaims
}

func grpcAuthFixture(t *testing.T) (*authctx.Verifier, string) {
	return grpcAuthFixtureForOrg(t, "triodelab")
}

func grpcAuthFixtureForOrg(t *testing.T, orgID string) (*authctx.Verifier, string) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences: []string{"capability-core"}, Issuer: "https://auth.example.test",
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}),
	})
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	claims := grpcAuthClaims{
		OrgID: orgID, ServiceID: "execution-core", PrincipalType: "service", Scopes: []string{authz.ReadScope},
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer: "https://auth.example.test", Subject: "execution-core", Audience: jwt.ClaimStrings{"capability-core"},
			IssuedAt: jwt.NewNumericDate(now.Add(-time.Minute)), NotBefore: jwt.NewNumericDate(now.Add(-time.Minute)),
			ExpiresAt: jwt.NewNumericDate(now.Add(time.Hour)),
		},
	}
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return verifier, raw
}

func verifiedGRPCContext(t *testing.T, orgID string) context.Context {
	t.Helper()
	verifier, raw := grpcAuthFixtureForOrg(t, orgID)
	interceptor := verifier.UnaryServerInterceptor(nil)
	incoming := metadata.NewIncomingContext(
		context.Background(),
		metadata.Pairs("authorization", "Bearer "+raw),
	)
	var verified context.Context
	_, err := interceptor(
		incoming,
		&mpv1.ListCapabilitiesRequest{},
		&grpc.UnaryServerInfo{FullMethod: mpv1.CapabilityCore_ListCapabilities_FullMethodName},
		func(ctx context.Context, _ any) (any, error) {
			verified = ctx
			return &mpv1.ListCapabilitiesResponse{}, nil
		},
	)
	if err != nil {
		t.Fatalf("derive verified gRPC context: %v", err)
	}
	if verified == nil {
		t.Fatal("verified gRPC context was not captured")
	}
	return verified
}

func TestCapabilityGRPCAuthenticationAndDerivedTenant(t *testing.T) {
	verifier, raw := grpcAuthFixture(t)
	interceptor := verifier.UnaryServerInterceptor(authz.AuthorizeGRPC)
	info := &grpc.UnaryServerInfo{FullMethod: mpv1.CapabilityCore_EvaluatePolicy_FullMethodName}
	handler := func(ctx context.Context, request any) (any, error) {
		return newTestServer().EvaluatePolicy(ctx, request.(*mpv1.EvaluatePolicyRequest))
	}

	if _, err := interceptor(context.Background(), &mpv1.EvaluatePolicyRequest{}, info, handler); status.Code(err) != codes.Unauthenticated {
		t.Fatalf("missing auth code = %v", status.Code(err))
	}
	malformed := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer invalid"))
	if _, err := interceptor(malformed, &mpv1.EvaluatePolicyRequest{}, info, handler); status.Code(err) != codes.Unauthenticated {
		t.Fatalf("malformed auth code = %v", status.Code(err))
	}
	forged := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+raw, "x-org-id", "org-b"))
	if _, err := interceptor(forged, &mpv1.EvaluatePolicyRequest{}, info, handler); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("forged header code = %v", status.Code(err))
	}
	valid := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+raw))
	if _, err := interceptor(valid, &mpv1.EvaluatePolicyRequest{OrgId: "org-b"}, info, handler); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("wrong tenant body code = %v", status.Code(err))
	}
	response, err := interceptor(valid, &mpv1.EvaluatePolicyRequest{
		CapabilityId: "cap.policy.round-robin", RunId: "run-a", AgentId: "agent-a", Scope: "global",
	}, info, handler)
	if err != nil {
		t.Fatalf("valid request failed: %v", err)
	}
	if response.(*mpv1.EvaluatePolicyResponse).GetDecision() == "" {
		t.Fatal("valid request did not reach tenant-derived policy evaluation")
	}
}
