package server

import (
	"context"
	"os"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

const defaultGrantTTL = 15 * time.Minute

// Server is the default implementation backed by an in-memory grant store.
type Server struct {
	mpv1.UnimplementedBrowserBrokerServer
	grants    *grant.Store
	baseURL   string
	principal func(context.Context) (authctx.Principal, error)
}

// NewServer constructs a Server with the provided grant store.
func NewServer(grants *grant.Store) *Server {
	baseURL := strings.TrimRight(os.Getenv("BROWSER_BROKER_BASE_URL"), "/")
	if baseURL == "" {
		baseURL = "https://browser-broker.local"
	}
	return &Server{grants: grants, baseURL: baseURL, principal: authz.Principal}
}

// AcquireGrant mints a new browser grant using the proto-defined session/mode contract.
func (s *Server) AcquireGrant(ctx context.Context, req *AcquireGrantRequest) (*AcquireGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "AcquireGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetSessionKey() == "" {
		return nil, status.Error(codes.InvalidArgument, "session_key is required")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	scopeURL, err := scopeURLForMode(req.GetMode())
	if err != nil {
		return nil, err
	}
	allowedDomains, err := grant.NormalizeAllowedDomains(req.GetAllowedDomains())
	if err != nil {
		return nil, status.Error(codes.InvalidArgument, "allowed_domains must contain bounded hostnames")
	}
	g, err := s.grants.Create(
		principal.OrganizationID,
		principal.ActorID,
		req.GetSessionKey(),
		scopeURL,
		allowedDomains,
		defaultGrantTTL,
	)
	if err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	return &AcquireGrantResponse{
		GrantId:        g.ID,
		Endpoint:       s.baseURL + "/grants/" + g.ID,
		ExpiresAt:      timestamppb.New(g.ExpiresAt),
		AllowedDomains: append([]string(nil), g.AllowedDomains...),
	}, nil
}

// RevokeGrant marks an existing grant as revoked.
func (s *Server) RevokeGrant(ctx context.Context, req *RevokeGrantRequest) (*RevokeGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "RevokeGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetGrantId() == "" {
		return nil, status.Error(codes.InvalidArgument, "grant_id is required")
	}
	if err := s.grants.RevokeScoped(req.GetGrantId(), principal.OrganizationID, authz.OwnerFilter(principal)); err != nil {
		telemetry.GrantsRevokedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsRevokedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "revoked")))
	return &RevokeGrantResponse{Revoked: true}, nil
}

// ValidateGrant checks that a grant exists, is not revoked, and is not expired.
func (s *Server) ValidateGrant(ctx context.Context, req *ValidateGrantRequest) (*ValidateGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ValidateGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetGrantId() == "" {
		return nil, status.Error(codes.InvalidArgument, "grant_id is required")
	}
	g, err := s.grants.GetScoped(req.GetGrantId(), principal.OrganizationID, authz.OwnerFilter(principal))
	if err != nil {
		telemetry.GrantsValidatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsValidatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "ok")))
	return &ValidateGrantResponse{
		GrantId:        g.ID,
		Active:         true,
		ExpiresAt:      timestamppb.New(g.ExpiresAt),
		AllowedDomains: append([]string(nil), g.AllowedDomains...),
	}, nil
}

// Health returns service health status.
func (s *Server) Health(ctx context.Context, _ *BrowserHealthRequest) (*BrowserHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	if _, err := s.principal(ctx); err != nil {
		return nil, err
	}
	return &BrowserHealthResponse{Status: "ok"}, nil
}

// Register wires the BrowserBroker service onto the provided gRPC server.
func Register(g *grpc.Server, impl mpv1.BrowserBrokerServer) {
	mpv1.RegisterBrowserBrokerServer(g, impl)
}

func scopeURLForMode(mode string) (string, error) {
	switch mode {
	case "", "cloud":
		return "browser://cloud", nil
	case "local":
		return "browser://local", nil
	default:
		return "", status.Errorf(codes.InvalidArgument, "unsupported browser mode %q", mode)
	}
}
