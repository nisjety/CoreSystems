package grpcserver

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	wikipb "github.com/triodelab/dataplane/gen/go/wiki/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
)

func TestAllGRPCMethodsEnforceSignedFourShapeMatrix(t *testing.T) {
	verifier, bearer := grpcTestIdentity(t)
	interceptor := authctx.UnaryServerInterceptor(verifier)

	routes := []struct {
		name, method string
		valid        any
		spoofed      any
	}{
		{"get page", "/wiki.v1.WikiService/GetPage", &wikipb.GetPageRequest{OrgId: "org-authorized"}, &wikipb.GetPageRequest{OrgId: "org-victim"}},
		{"get page by path", "/wiki.v1.WikiService/GetPageByPath", &wikipb.GetPageByPathRequest{OrgId: "org-authorized"}, &wikipb.GetPageByPathRequest{OrgId: "org-victim"}},
		{"list page versions", "/wiki.v1.WikiService/ListPageVersions", &wikipb.ListPageVersionsRequest{OrgId: "org-authorized"}, &wikipb.ListPageVersionsRequest{OrgId: "org-victim"}},
		{"get page sources", "/wiki.v1.WikiService/GetPageSources", &wikipb.GetPageSourcesRequest{OrgId: "org-authorized"}, &wikipb.GetPageSourcesRequest{OrgId: "org-victim"}},
		{"list maintenance", "/wiki.v1.WikiService/ListMaintenanceIssues", &wikipb.ListMaintenanceIssuesRequest{OrgId: "org-authorized"}, &wikipb.ListMaintenanceIssuesRequest{OrgId: "org-victim"}},
		{"get backlinks", "/wiki.v1.WikiService/GetBacklinks", &wikipb.GetBacklinksRequest{OrgId: "org-authorized"}, &wikipb.GetBacklinksRequest{OrgId: "org-victim"}},
		{"create page", "/wiki.v1.WikiService/CreatePage", &wikipb.CreatePageRequest{OrgId: "org-authorized"}, &wikipb.CreatePageRequest{OrgId: "org-victim"}},
		{"update page", "/wiki.v1.WikiService/UpdatePageVersion", &wikipb.UpdatePageVersionRequest{OrgId: "org-authorized"}, &wikipb.UpdatePageVersionRequest{OrgId: "org-victim"}},
		{"submit proposal", "/wiki.v1.WikiService/SubmitProposal", &wikipb.SubmitProposalRequest{OrgId: "org-authorized"}, &wikipb.SubmitProposalRequest{OrgId: "org-victim"}},
		{"review proposal", "/wiki.v1.WikiService/ReviewProposal", &wikipb.ReviewProposalRequest{OrgId: "org-authorized"}, &wikipb.ReviewProposalRequest{OrgId: "org-victim"}},
	}

	for _, route := range routes {
		shapes := []struct {
			name   string
			ctx    context.Context
			req    any
			want   codes.Code
			called bool
		}{
			{name: "no auth", ctx: context.Background(), req: route.valid, want: codes.Unauthenticated},
			{name: "forged org metadata", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("x-org-id", "org-victim")), req: route.valid, want: codes.Unauthenticated},
			{name: "valid signed bearer", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer)), req: route.valid, want: codes.OK, called: true},
			{name: "valid bearer plus spoofed org", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer)), req: route.spoofed, want: codes.PermissionDenied},
		}
		for _, shape := range shapes {
			t.Run(route.name+"/"+shape.name, func(t *testing.T) {
				called := false
				_, err := interceptor(shape.ctx, shape.req, &grpc.UnaryServerInfo{FullMethod: route.method}, func(ctx context.Context, _ any) (any, error) {
					called = true
					claims, ok := authctx.FromContext(ctx)
					if !ok || claims.OrgID != "org-authorized" {
						t.Fatalf("handler claims = (%+v, %v)", claims, ok)
					}
					return struct{}{}, nil
				})
				if got := status.Code(err); got != shape.want {
					t.Fatalf("code = %s, want %s; err=%v", got, shape.want, err)
				}
				if called != shape.called {
					t.Fatalf("handler called = %v, want %v", called, shape.called)
				}
			})
		}
	}
}

func grpcTestIdentity(t *testing.T) (*authctx.Verifier, string) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	encoded, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audience: "data-plane", Issuer: "https://auth.test/issuer",
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: encoded}),
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	now := time.Now()
	claims := jwt.MapClaims{
		"iss": "https://auth.test/issuer", "aud": "data-plane",
		"sub": "user-authorized", "user_id": "user-authorized", "org_id": "org-authorized",
		"scopes": []string{"wiki.read", "wiki.write", "wiki.approve"},
		"iat":    now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(), "exp": now.Add(time.Hour).Unix(),
	}
	bearer, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}
	return verifier, bearer
}
