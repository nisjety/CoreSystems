package clients

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"math"
	"net"
	"os"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

// SessionRunActionAuthorityClient is Control's narrow, read-only Session Core
// client. It uses a deployment-issued bearer restricted to the two narrow
// Session Core authority scopes used here (run-action and scheduled-step
// authority); it must never be replaced by a broad Model service token or a
// browser credential.
type SessionRunActionAuthorityClient struct {
	client mpv1.RunServiceClient
	conn   *grpc.ClientConn
	bearer string
}

// SessionRunActionAuthorityTransport controls Control's authenticated hop to
// Session Core. TLS uses the platform trust store unless TLSCAFile supplies a
// deployment CA bundle. Plaintext is deliberately limited to an explicit
// IP-loopback development endpoint; it cannot be used for a Docker service or
// a private/remote hostname.
type SessionRunActionAuthorityTransport struct {
	TLSCAFile             string
	TLSServerName         string
	AllowInsecureLoopback bool
}

// NewSessionRunActionAuthorityClient returns nil when the deployment has not
// configured the dedicated Control-to-Session credential. That intentionally
// leaves owner-action issuance unavailable rather than accepting a fallback.
func NewSessionRunActionAuthorityClient(addr, bearer string, transport SessionRunActionAuthorityTransport) (*SessionRunActionAuthorityClient, error) {
	addr = strings.TrimSpace(addr)
	bearer = strings.TrimSpace(bearer)
	if addr == "" || bearer == "" {
		return nil, nil
	}
	if strings.HasPrefix(strings.ToLower(bearer), "bearer ") {
		return nil, fmt.Errorf("the Control Session Core bearer must not include an authorization scheme")
	}
	transportCredentials, err := sessionRunActionAuthorityTransportCredentials(addr, transport)
	if err != nil {
		return nil, err
	}
	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(transportCredentials))
	if err != nil {
		return nil, fmt.Errorf("dial Session Core run action authority: %w", err)
	}
	return &SessionRunActionAuthorityClient{
		client: mpv1.NewRunServiceClient(conn), conn: conn, bearer: bearer,
	}, nil
}

func sessionRunActionAuthorityTransportCredentials(addr string, transport SessionRunActionAuthorityTransport) (credentials.TransportCredentials, error) {
	addr = strings.TrimSpace(addr)
	if transport.AllowInsecureLoopback {
		if !isIPLoopbackGRPCTarget(addr) {
			return nil, fmt.Errorf("insecure Control-to-Session authority transport is permitted only for an explicit IP-loopback development endpoint")
		}
		return insecure.NewCredentials(), nil
	}

	tlsConfig := &tls.Config{MinVersion: tls.VersionTLS12}
	if serverName := strings.TrimSpace(transport.TLSServerName); serverName != "" {
		tlsConfig.ServerName = serverName
	}
	if caFile := strings.TrimSpace(transport.TLSCAFile); caFile != "" {
		pem, err := os.ReadFile(caFile)
		if err != nil {
			return nil, fmt.Errorf("read Session Core run authority TLS CA: %w", err)
		}
		roots := x509.NewCertPool()
		if !roots.AppendCertsFromPEM(pem) {
			return nil, fmt.Errorf("the Session Core run authority TLS CA is invalid")
		}
		tlsConfig.RootCAs = roots
	}
	return credentials.NewTLS(tlsConfig), nil
}

func isIPLoopbackGRPCTarget(target string) bool {
	host, _, err := net.SplitHostPort(target)
	if err != nil {
		return false
	}
	ip := net.ParseIP(strings.TrimSpace(host))
	return ip != nil && ip.IsLoopback()
}

func (c *SessionRunActionAuthorityClient) Close() error {
	if c == nil || c.conn == nil {
		return nil
	}
	return c.conn.Close()
}

// ResolveRunActionAuthority returns only immutable identifiers and policy
// references. The wire response has no transcript, goal, tool input/output,
// approval payload, or credential fields; an unresolved result remains an
// error so the HTTP issuer does not turn an absent run into permissive state.
func (c *SessionRunActionAuthorityClient) ResolveRunActionAuthority(ctx context.Context, runID, orgID string) (spaces.RunActionAuthority, error) {
	if c == nil || c.client == nil || strings.TrimSpace(c.bearer) == "" {
		return spaces.RunActionAuthority{}, fmt.Errorf("the Session Core run action authority client is not configured")
	}
	runID = strings.TrimSpace(runID)
	orgID = strings.TrimSpace(orgID)
	if runID == "" || orgID == "" {
		return spaces.RunActionAuthority{}, fmt.Errorf("run and organization are required")
	}
	response, err := c.client.ResolveRunActionAuthority(
		metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+c.bearer),
		&mpv1.ResolveRunActionAuthorityRequest{RunId: runID, OrgId: orgID},
	)
	if err != nil {
		return spaces.RunActionAuthority{}, fmt.Errorf("resolve Session Core run action authority: %w", err)
	}
	return runActionAuthorityFromResponse(response)
}

// ResolveScheduledStepAuthority asks Session Core to prove that the exact
// prepared scheduled run/fire/template is still active. The request carries no
// subject; Session Core derives it from the durable prepared-run metadata.
func (c *SessionRunActionAuthorityClient) ResolveScheduledStepAuthority(ctx context.Context, intent spaces.ScheduledStepIntent) (spaces.ScheduledStepAuthority, error) {
	if c == nil || c.client == nil || strings.TrimSpace(c.bearer) == "" {
		return spaces.ScheduledStepAuthority{}, fmt.Errorf("the Session Core scheduled step authority client is not configured")
	}
	if err := intent.ValidateAuthorityRequest(); err != nil {
		return spaces.ScheduledStepAuthority{}, fmt.Errorf("invalid scheduled step authority request: %w", err)
	}
	response, err := c.client.ResolveScheduledStepAuthority(
		metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+c.bearer),
		&mpv1.ResolveScheduledStepAuthorityRequest{
			RunId: intent.RunID, ThreadId: intent.ThreadID, OrgId: intent.OrgID,
			ScheduleId: intent.ScheduleID, FireKey: intent.FireKey,
			TemplateDigest: intent.TemplateDigest, PolicyDigest: intent.PolicyDigest,
			StepId: intent.StepID, StepIndex: intent.StepIndex, IdempotencyKey: intent.IdempotencyKey,
		},
	)
	if err != nil {
		return spaces.ScheduledStepAuthority{}, fmt.Errorf("resolve Session Core scheduled step authority: %w", err)
	}
	return scheduledStepAuthorityFromResponse(response)
}

func scheduledStepAuthorityFromResponse(response *mpv1.ResolveScheduledStepAuthorityResponse) (spaces.ScheduledStepAuthority, error) {
	if response == nil || !response.GetResolved() {
		return spaces.ScheduledStepAuthority{}, fmt.Errorf("the requested scheduled step authority was not resolved by Session Core")
	}
	authority := spaces.ScheduledStepAuthority{
		RunID: response.GetRunId(), ThreadID: response.GetThreadId(), OrgID: response.GetOrgId(),
		SubjectID: response.GetSubjectId(), SpaceRef: response.GetSpaceId(), ScheduleID: response.GetScheduleId(),
		FireKey: response.GetFireKey(), TemplateDigest: response.GetTemplateDigest(),
		PolicyDigest: response.GetPolicyDigest(), StepID: response.GetStepId(), StepIndex: response.GetStepIndex(),
		IdempotencyKey: response.GetIdempotencyKey(), RunStatus: response.GetRunStatus(),
	}
	if err := authority.Validate(); err != nil {
		return spaces.ScheduledStepAuthority{}, err
	}
	return authority, nil
}

func runActionAuthorityFromResponse(response *mpv1.ResolveRunActionAuthorityResponse) (spaces.RunActionAuthority, error) {
	if response == nil || !response.GetResolved() {
		return spaces.RunActionAuthority{}, fmt.Errorf("the requested run authority was not resolved by Session Core")
	}
	if response.GetRecipientAudienceRevision() > math.MaxInt64 || response.GetAuthorityRevision() > math.MaxInt64 {
		return spaces.RunActionAuthority{}, fmt.Errorf("the Session Core run action authority revision is out of range")
	}
	authority := spaces.RunActionAuthority{
		RunID: response.GetRunId(), OrgID: response.GetOrgId(), SubjectID: response.GetSubjectId(),
		ThreadID: response.GetThreadId(), SpaceRef: response.GetSpaceId(),
		RecipientAudienceRef:      response.GetRecipientAudienceRef(),
		RecipientAudienceRevision: int64(response.GetRecipientAudienceRevision()),
		RecipientAudienceHash:     response.GetRecipientAudienceHash(), PrivacyPolicyRef: response.GetPrivacyPolicyRef(),
		RunContextAuthorizationRef: response.GetThreadResourceAuthorizationRef(),
		AuthorityRevision:          int64(response.GetAuthorityRevision()),
		RunStatus:                  response.GetRunStatus(),
	}
	if err := authority.Validate(); err != nil {
		return spaces.RunActionAuthority{}, fmt.Errorf("invalid Session Core run action authority: %w", err)
	}
	return authority, nil
}
