package orchestration

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/sdk/client"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/types/known/structpb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

// Scopes a service principal needs to start durable work.
const (
	// ScopeWorkflowStart authorizes starting an allowlisted workflow inside the
	// principal's own organization.
	ScopeWorkflowStart = "orchestration:workflow:start"
	// ScopeWorkflowStartGlobal additionally authorizes workflows that mutate
	// state shared beyond one organization (the skill registry).
	ScopeWorkflowStartGlobal = "orchestration:workflow:start:global"
)

// InternalTokenMetadataKey is the gRPC metadata key carrying the Model-Plane-
// local shared service secret. It mirrors the existing MCP_OAUTH_SERVICE_TOKEN
// pattern: an internal-only credential for a hop that has no live per-user
// bearer to present.
const InternalTokenMetadataKey = "x-model-plane-internal-token"

// Caller is the authenticated identity behind one StartWorkflow call.
type Caller struct {
	// OrgID / UserID are populated only for JWT principals. The internal
	// shared-secret path carries no identity of its own and leaves them empty.
	OrgID  string
	UserID string
	// Service marks a non-interactive principal (JWT principal_type=service, or
	// the internal shared secret).
	Service bool
	Scopes  []string
	// RetentionPolicyPresent reports whether the issuer stamped an explicit
	// retention posture. False means "unknown", which is never treated as
	// "retention allowed".
	RetentionPolicyPresent bool
	ZDR                    bool
	// Internal marks the Model-Plane-local shared-secret path.
	Internal bool
}

func (c Caller) hasScope(scope string) bool {
	for _, s := range c.Scopes {
		if s == scope {
			return true
		}
	}
	return false
}

// StartWorkflowAuth authenticates StartWorkflow callers.
//
// It deliberately does NOT run as a server-wide gRPC interceptor. Every other
// RPC on this server is a proxy that forwards the caller's credential to
// session-core, which is the authority for it; verifying those tokens locally
// against an orchestrator audience would reject credentials session-core
// happily accepts today and break the gateway's plan/approval reads. So the new
// mutating RPC authenticates in its own handler and the read proxies are left
// byte-for-byte unchanged.
type StartWorkflowAuth struct {
	verifier      *authctx.Verifier
	internalToken string
	internalOrgs  []string
}

// NewStartWorkflowAuth builds the authenticator.
//
// verifier may be nil (Auth Core trust material not configured) and
// internalToken/internalOrgs may be empty (internal path disabled). When BOTH
// are absent StartWorkflow refuses every call with codes.Unavailable — it never
// degrades to an unauthenticated mode.
//
// internalOrgs is a mandatory, explicit allowlist for the shared-secret path:
// there is no wildcard, so a leaked secret still cannot start work outside the
// organizations the deployment named.
func NewStartWorkflowAuth(verifier *authctx.Verifier, internalToken string, internalOrgs []string) *StartWorkflowAuth {
	token := strings.TrimSpace(internalToken)
	orgs := make([]string, 0, len(internalOrgs))
	for _, o := range internalOrgs {
		if o = strings.TrimSpace(o); o != "" {
			orgs = append(orgs, o)
		}
	}
	// Half a configuration is no configuration: a secret with no org allowlist
	// would be an unbounded credential, so the mode stays off.
	if token == "" || len(orgs) == 0 {
		token, orgs = "", nil
	}
	return &StartWorkflowAuth{verifier: verifier, internalToken: token, internalOrgs: orgs}
}

// Configured reports whether any credential source is available.
func (a *StartWorkflowAuth) Configured() bool {
	return a != nil && (a.verifier != nil || a.internalToken != "")
}

func (a *StartWorkflowAuth) internalOrgAllowed(orgID string) bool {
	for _, o := range a.internalOrgs {
		if o == orgID {
			return true
		}
	}
	return false
}

// Authenticate resolves the caller from gRPC metadata. The internal secret is
// checked first and, when presented, is decisive: a mismatch fails closed
// instead of falling through to the bearer path.
func (a *StartWorkflowAuth) Authenticate(ctx context.Context) (Caller, error) {
	if !a.Configured() {
		return Caller{}, status.Error(codes.Unavailable, "workflow start is not configured")
	}
	md, _ := metadata.FromIncomingContext(ctx)

	if presented := firstMD(md, InternalTokenMetadataKey); presented != "" {
		if a.internalToken == "" {
			return Caller{}, status.Error(codes.Unauthenticated, "internal service credential is not accepted")
		}
		if subtle.ConstantTimeCompare([]byte(presented), []byte(a.internalToken)) != 1 {
			return Caller{}, status.Error(codes.Unauthenticated, "internal service credential is invalid")
		}
		return Caller{Service: true, Internal: true}, nil
	}

	if a.verifier == nil {
		return Caller{}, status.Error(codes.Unavailable, "bearer authentication is not configured")
	}
	raw := bearerToken(firstMD(md, "authorization"))
	if raw == "" {
		return Caller{}, status.Error(codes.Unauthenticated, "authentication required")
	}
	principal, err := a.verifier.Verify(raw)
	if err != nil {
		return Caller{}, status.Error(codes.Unauthenticated, "invalid authentication token")
	}
	// Reject forged identity headers exactly like authctx's own interceptors do.
	if conflictsMD(firstMD(md, "x-org-id"), principal.OrganizationID) ||
		conflictsMD(firstMD(md, "x-user-id"), principal.ActorID) {
		return Caller{}, status.Error(codes.PermissionDenied, "identity context does not match verified token")
	}
	caller := Caller{
		OrgID:                  principal.OrganizationID,
		Service:                principal.PrincipalType == "service",
		Scopes:                 principal.Scopes,
		RetentionPolicyPresent: principal.RetentionPolicyPresent,
		ZDR:                    principal.ZeroDataRetention,
	}
	if !caller.Service {
		caller.UserID = principal.ActorID
	}
	return caller, nil
}

// StartOptions is the narrow slice of Temporal start options this package needs.
type StartOptions struct {
	WorkflowID   string
	WorkflowType string
	TaskQueue    string
}

// StartedWorkflow identifies the execution a start resolved to — either the one
// it created or, on a retry, the one that was already running.
type StartedWorkflow struct {
	WorkflowID    string
	TemporalRunID string
}

// WorkflowStarter starts a workflow by type name on a task queue. Narrow on
// purpose: the handler needs nothing else from Temporal, and a small interface
// keeps the authorization logic unit-testable without a Temporal server.
type WorkflowStarter interface {
	StartWorkflow(ctx context.Context, opts StartOptions, arg any) (StartedWorkflow, error)
}

// TemporalStarter adapts a Temporal client to WorkflowStarter.
type TemporalStarter struct{ c client.Client }

// NewTemporalStarter wraps a Temporal client.
func NewTemporalStarter(c client.Client) *TemporalStarter { return &TemporalStarter{c: c} }

// StartWorkflow starts (or re-attaches to) the deterministic workflow id.
//
// WORKFLOW_ID_CONFLICT_POLICY_USE_EXISTING is what makes a retry safe: when the
// id is already running, the server returns the existing execution instead of
// starting a second one or erroring. Combined with the deterministic id this
// gives at-most-one run per (type, org, anchor) without any dedup table.
func (t *TemporalStarter) StartWorkflow(ctx context.Context, opts StartOptions, arg any) (StartedWorkflow, error) {
	if t == nil || t.c == nil {
		return StartedWorkflow{}, errors.New("orchestration: temporal client not configured")
	}
	run, err := t.c.ExecuteWorkflow(ctx, client.StartWorkflowOptions{
		ID:                       opts.WorkflowID,
		TaskQueue:                opts.TaskQueue,
		WorkflowIDConflictPolicy: enumspb.WORKFLOW_ID_CONFLICT_POLICY_USE_EXISTING,
	}, opts.WorkflowType, arg)
	if err != nil {
		return StartedWorkflow{}, err
	}
	return StartedWorkflow{WorkflowID: run.GetID(), TemporalRunID: run.GetRunID()}, nil
}

// WorkflowStartService turns an authenticated, allowlisted StartWorkflow
// request into one durable Temporal execution. It implements
// mpv1.OrchestratorWorkflowServiceServer and is registered on the same gRPC
// listener as the OrchestrationCoreService proxy — a second service on the
// existing port, not a second port.
type WorkflowStartService struct {
	mpv1.UnimplementedOrchestratorWorkflowServiceServer
	auth      *StartWorkflowAuth
	starter   WorkflowStarter
	taskQueue string
}

// NewWorkflowStartService wires the authenticator, starter, and task queue.
func NewWorkflowStartService(auth *StartWorkflowAuth, starter WorkflowStarter, taskQueue string) *WorkflowStartService {
	return &WorkflowStartService{auth: auth, starter: starter, taskQueue: taskQueue}
}

// StartWorkflow is the gRPC entry point. Unlike every other RPC this service
// hosts, it is not a session-core proxy: it starts durable Temporal work
// locally, so it authenticates and authorizes the caller itself instead of
// forwarding a credential upstream.
func (s *WorkflowStartService) StartWorkflow(
	ctx context.Context,
	req *mpv1.StartWorkflowRequest,
) (*mpv1.StartWorkflowResponse, error) {
	return s.Start(ctx, req)
}

// Start authenticates, authorizes, validates, and starts one workflow.
func (s *WorkflowStartService) Start(
	ctx context.Context,
	req *mpv1.StartWorkflowRequest,
) (*mpv1.StartWorkflowResponse, error) {
	if s == nil || s.starter == nil {
		return nil, status.Error(codes.Unavailable, "workflow start is not configured")
	}
	if req == nil {
		return nil, status.Error(codes.InvalidArgument, "request is required")
	}

	caller, err := s.auth.Authenticate(ctx)
	if err != nil {
		return nil, err
	}

	spec, ok := LookupWorkflow(req.GetWorkflowType())
	if !ok {
		// Name the allowlist rather than echoing the rejected string back.
		return nil, status.Errorf(codes.InvalidArgument,
			"workflow_type is not allowlisted; allowed: %s", strings.Join(AllowedWorkflowTypes(), ", "))
	}

	orgID, err := s.resolveOrg(caller, req.GetOrgId())
	if err != nil {
		return nil, err
	}
	userID, err := resolveUser(caller, req.GetUserId())
	if err != nil {
		return nil, err
	}
	if err := authorizeRetention(caller, spec); err != nil {
		return nil, err
	}
	if err := authorizePolicy(caller, spec); err != nil {
		return nil, err
	}

	runID := strings.TrimSpace(req.GetRunId())
	if spec.RequiresRunID || runID != "" {
		if verr := ValidateRunID(req.GetRunId()); verr != nil {
			return nil, status.Error(codes.InvalidArgument, verr.Error())
		}
		runID = req.GetRunId()
	}

	anchor := strings.TrimSpace(req.GetIdempotencyKey())
	if anchor == "" {
		anchor = runID
	} else if verr := ValidateRunID(anchor); verr != nil {
		return nil, status.Error(codes.InvalidArgument,
			"idempotency_key must be a single NATS subject token (no '.', whitespace, '*' or '>')")
	}
	if anchor == "" {
		return nil, status.Error(codes.InvalidArgument,
			"run_id or idempotency_key is required to derive a deterministic workflow id")
	}

	rawInput, err := structToJSON(req.GetInput())
	if err != nil {
		return nil, status.Error(codes.InvalidArgument, err.Error())
	}
	arg, err := spec.BuildInput(rawInput, Tenancy{
		OrgID:  orgID,
		UserID: userID,
		RunID:  runID,
		ZDR:    caller.ZDR,
	})
	if err != nil {
		return nil, status.Error(codes.InvalidArgument, err.Error())
	}

	started, err := s.starter.StartWorkflow(ctx, StartOptions{
		WorkflowID: WorkflowID(spec.Type, orgID, anchor),
		// Always the allowlist's canonical type, never the request string.
		WorkflowType: spec.Type,
		TaskQueue:    s.taskQueue,
	}, arg)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "start workflow: %v", err)
	}
	return &mpv1.StartWorkflowResponse{
		WorkflowId:    started.WorkflowID,
		TemporalRunId: started.TemporalRunID,
		WorkflowType:  spec.Type,
	}, nil
}

// resolveOrg derives the server-authoritative tenant.
//
// JWT callers: the tenant is the token's org; a conflicting request org_id is
// denied rather than coerced, so a mismatch is never silently "corrected" into
// a successful start against the wrong tenant.
//
// Internal callers: the shared secret carries no tenant, so the request must
// name one and it must appear in the deployment's explicit allowlist.
func (s *WorkflowStartService) resolveOrg(caller Caller, requestedOrg string) (string, error) {
	requested := strings.TrimSpace(requestedOrg)
	if caller.Internal {
		if requested == "" {
			return "", status.Error(codes.InvalidArgument, "org_id is required for internal service callers")
		}
		if !s.auth.internalOrgAllowed(requested) {
			return "", status.Error(codes.PermissionDenied, "org_id is not in the internal service allowlist")
		}
		return requested, nil
	}
	if caller.OrgID == "" {
		return "", status.Error(codes.PermissionDenied, "verified caller has no organization")
	}
	if requested != "" && requested != caller.OrgID {
		return "", status.Error(codes.PermissionDenied, "org_id does not match the verified caller")
	}
	return caller.OrgID, nil
}

// resolveUser derives the acting viewer.
//
// A user principal always acts as itself. Service and internal principals must
// leave user_id empty: user_id is threaded into ExecuteStep and narrows
// retrieval to that viewer's visible set, so honouring a service-supplied value
// would let a workload read another person's documents. Org-scoped is the only
// posture available to a workload until a real delegation contract exists.
func resolveUser(caller Caller, requestedUser string) (string, error) {
	requested := strings.TrimSpace(requestedUser)
	if caller.Service {
		if requested != "" {
			return "", status.Error(codes.PermissionDenied,
				"user_id is not accepted from a service principal; the run is org-scoped")
		}
		return "", nil
	}
	if requested != "" && requested != caller.UserID {
		return "", status.Error(codes.PermissionDenied, "user_id does not match the verified caller")
	}
	return caller.UserID, nil
}

// authorizeRetention enforces the zero-data-retention floor.
//
// A JWT caller must carry an explicit posture — an absent posture is "unknown",
// and unknown is not permission. Workflows marked DeniesZDR persist derived
// content that outlives the run (consolidated memory, promoted skills), so a
// ZDR caller is refused instead of having its posture quietly dropped. The
// internal shared-secret path has no signed posture at all and therefore can
// never start those workflows.
func authorizeRetention(caller Caller, spec WorkflowSpec) error {
	if caller.Internal {
		if spec.DeniesZDR {
			return status.Error(codes.PermissionDenied,
				"this workflow persists derived content and requires a signed retention posture")
		}
		return nil
	}
	if !caller.RetentionPolicyPresent {
		return status.Error(codes.PermissionDenied, "verified caller has no explicit retention posture")
	}
	if spec.DeniesZDR && caller.ZDR {
		return status.Error(codes.PermissionDenied,
			"zero-data-retention callers cannot start a workflow that persists derived content")
	}
	return nil
}

// authorizePolicy applies the per-policy caller rules.
func authorizePolicy(caller Caller, spec WorkflowSpec) error {
	switch spec.Policy {
	case PolicyRunScoped:
		if caller.Internal {
			return nil
		}
		if !caller.Service {
			return nil
		}
		if caller.hasScope(ScopeWorkflowStart) {
			return nil
		}
		return status.Error(codes.PermissionDenied, "service principal lacks "+ScopeWorkflowStart)
	case PolicyMaintenance:
		if caller.Internal {
			return nil
		}
		if !caller.Service {
			return status.Error(codes.PermissionDenied,
				"maintenance workflows are service-initiated only")
		}
		if caller.hasScope(ScopeWorkflowStart) {
			return nil
		}
		return status.Error(codes.PermissionDenied, "service principal lacks "+ScopeWorkflowStart)
	case PolicyGlobalRegistry:
		if caller.Internal {
			return status.Error(codes.PermissionDenied,
				"the internal service credential cannot start registry-wide workflows")
		}
		if !caller.Service {
			return status.Error(codes.PermissionDenied,
				"registry-wide workflows are service-initiated only")
		}
		if caller.hasScope(ScopeWorkflowStart) && caller.hasScope(ScopeWorkflowStartGlobal) {
			return nil
		}
		return status.Error(codes.PermissionDenied,
			"service principal lacks "+ScopeWorkflowStartGlobal)
	default:
		return status.Error(codes.Internal, "workflow has no start policy")
	}
}

// WorkflowID derives the deterministic Temporal workflow id.
//
// Shape: mp-wf/<Type>/<sha256(org_id)[:32]>/<anchor>
//
//   - Deterministic, so a retried or redelivered start resolves to the same id
//     and (with USE_EXISTING) cannot double-start a run.
//   - Type is the allowlist constant, so the id is readable in the Temporal UI.
//   - The org is hashed rather than interpolated: org ids come from a JWT claim
//     and are not constrained by this service, so hashing keeps the id
//     bounded-length and free of separator injection while still guaranteeing
//     two organizations can never collide on one workflow id.
//   - The anchor (run_id, or idempotency_key when supplied) is validated to a
//     single safe token before it reaches this function, so it stays readable.
func WorkflowID(workflowType, orgID, anchor string) string {
	sum := sha256.Sum256([]byte(orgID))
	return fmt.Sprintf("mp-wf/%s/%s/%s", workflowType, hex.EncodeToString(sum[:])[:32], anchor)
}

// structToJSON renders the request's protobuf Struct as canonical JSON so the
// per-workflow builders can decode it strictly. A nil Struct yields nil, which
// decodeInput treats as "no input supplied".
func structToJSON(s *structpb.Struct) (json.RawMessage, error) {
	if s == nil {
		return nil, nil
	}
	data, err := protojson.Marshal(s)
	if err != nil {
		return nil, fmt.Errorf("input is not valid JSON: %w", err)
	}
	return data, nil
}

func firstMD(md metadata.MD, key string) string {
	values := md.Get(key)
	if len(values) == 0 {
		return ""
	}
	return strings.TrimSpace(values[0])
}

func bearerToken(header string) string {
	parts := strings.Fields(header)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return strings.TrimSpace(parts[1])
}

func conflictsMD(unverified, verified string) bool {
	unverified = strings.TrimSpace(unverified)
	return unverified != "" && unverified != verified
}
