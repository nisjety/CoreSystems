package orchestration

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"errors"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/structpb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/workflows"
)

const (
	testIssuer   = "https://auth.test/api/convex-auth"
	testAudience = "orchestrator-core"
	testQueue    = "model-plane-orchestrator"
	internalTok  = "internal-shared-secret"
)

// --- fakes -----------------------------------------------------------------

type recordedStart struct {
	opts StartOptions
	arg  any
}

type fakeStarter struct {
	starts []recordedStart
	err    error
}

func (f *fakeStarter) StartWorkflow(_ context.Context, opts StartOptions, arg any) (StartedWorkflow, error) {
	if f.err != nil {
		return StartedWorkflow{}, f.err
	}
	f.starts = append(f.starts, recordedStart{opts: opts, arg: arg})
	return StartedWorkflow{WorkflowID: opts.WorkflowID, TemporalRunID: "temporal-run-1"}, nil
}

type testClaims struct {
	OrgID         string   `json:"org_id,omitempty"`
	UserID        string   `json:"user_id,omitempty"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type,omitempty"`
	Scopes        []string `json:"scopes,omitempty"`
	ZDR           *bool    `json:"zdr,omitempty"`
	jwt.RegisteredClaims
}

func testAuth(t *testing.T) (*StartWorkflowAuth, *rsa.PrivateKey) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	require.NoError(t, err)
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	require.NoError(t, err)
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{testAudience},
		Issuer:       testIssuer,
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}),
	})
	require.NoError(t, err)
	return NewStartWorkflowAuth(verifier, internalTok, []string{"org-allowed"}), key
}

func sign(t *testing.T, key *rsa.PrivateKey, c testClaims) string {
	t.Helper()
	if c.Issuer == "" {
		c.Issuer = testIssuer
	}
	if c.Audience == nil {
		c.Audience = jwt.ClaimStrings{testAudience}
	}
	if c.Subject == "" {
		if c.PrincipalType == "service" {
			c.Subject = c.ServiceID
		} else {
			c.Subject = c.UserID
		}
	}
	now := time.Now()
	if c.IssuedAt == nil {
		c.IssuedAt = jwt.NewNumericDate(now.Add(-time.Minute))
	}
	if c.NotBefore == nil {
		c.NotBefore = jwt.NewNumericDate(now.Add(-time.Minute))
	}
	if c.ExpiresAt == nil {
		c.ExpiresAt = jwt.NewNumericDate(now.Add(time.Hour))
	}
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, c).SignedString(key)
	require.NoError(t, err)
	return raw
}

func boolPtr(b bool) *bool { return &b }

// userClaims is a normal interactive principal with an explicit retention
// posture (zdr:false), which is what Auth Core stamps for ordinary orgs.
func userClaims(org, user string) testClaims {
	return testClaims{OrgID: org, UserID: user, PrincipalType: "user", ZDR: boolPtr(false)}
}

func serviceClaims(org string, scopes []string, zdr bool) testClaims {
	return testClaims{
		OrgID:         org,
		ServiceID:     "svc-dispatcher",
		PrincipalType: "service",
		Scopes:        scopes,
		ZDR:           boolPtr(zdr),
	}
}

func bearerCtx(token string) context.Context {
	return metadata.NewIncomingContext(context.Background(),
		metadata.Pairs("authorization", "Bearer "+token))
}

func internalCtx(token string) context.Context {
	return metadata.NewIncomingContext(context.Background(),
		metadata.Pairs(InternalTokenMetadataKey, token))
}

func mustStruct(t *testing.T, m map[string]any) *structpb.Struct {
	t.Helper()
	s, err := structpb.NewStruct(m)
	require.NoError(t, err)
	return s
}

func codeOf(err error) codes.Code {
	return status.Code(err)
}

// --- allowlist -------------------------------------------------------------

func TestAllowlistExcludesAutoresearch(t *testing.T) {
	// Autoresearch's budget guard is driven by a fabricated per-step cost, so
	// exposing it would advertise a spending cap that is not enforced.
	_, ok := LookupWorkflow("AutoresearchWorkflow")
	assert.False(t, ok, "AutoresearchWorkflow must not be startable")
	assert.NotContains(t, AllowedWorkflowTypes(), "AutoresearchWorkflow")
}

func TestUnknownWorkflowTypeIsRejectedByName(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	for _, name := range []string{
		"", "interactiverunsupervision", "InteractiveRunSupervision ",
		"DropDatabaseWorkflow", "../InteractiveRunSupervision",
	} {
		_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
			&mpv1.StartWorkflowRequest{WorkflowType: name, RunId: "run-1"})
		require.Error(t, err, "type %q must be rejected", name)
		assert.Equal(t, codes.InvalidArgument, codeOf(err), "type %q", name)
	}
	assert.Empty(t, starter.starts, "no unvetted type may reach Temporal")
}

func TestStartAlwaysPassesTheCanonicalTypeToTemporal(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			RunId:        "run-1",
			Input:        mustStruct(t, map[string]any{"goal": "do the thing"}),
		})
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	assert.Equal(t, "InteractiveRunSupervision", starter.starts[0].opts.WorkflowType)
	assert.Equal(t, testQueue, starter.starts[0].opts.TaskQueue)
}

// --- authentication --------------------------------------------------------

func TestUnauthenticatedStartIsRefused(t *testing.T) {
	auth, _ := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	req := &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision",
		RunId:        "run-1",
		Input:        mustStruct(t, map[string]any{"goal": "g"}),
	}
	_, err := svc.Start(context.Background(), req)
	assert.Equal(t, codes.Unauthenticated, codeOf(err))

	_, err = svc.Start(bearerCtx("not-a-jwt"), req)
	assert.Equal(t, codes.Unauthenticated, codeOf(err))

	assert.Empty(t, starter.starts)
}

func TestNoCredentialSourceMeansUnavailableNotOpen(t *testing.T) {
	svc := NewWorkflowStartService(NewStartWorkflowAuth(nil, "", nil), &fakeStarter{}, testQueue)
	_, err := svc.Start(context.Background(), &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision", RunId: "run-1",
	})
	assert.Equal(t, codes.Unavailable, codeOf(err))
}

func TestInternalTokenRequiresAnOrgAllowlist(t *testing.T) {
	// A secret with no org allowlist would be an unbounded credential, so the
	// whole mode must stay off rather than accept an unbounded caller.
	auth := NewStartWorkflowAuth(nil, internalTok, nil)
	assert.False(t, auth.Configured())

	svc := NewWorkflowStartService(auth, &fakeStarter{}, testQueue)
	_, err := svc.Start(internalCtx(internalTok), &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision", OrgId: "org-allowed", RunId: "run-1",
	})
	assert.Equal(t, codes.Unavailable, codeOf(err))
}

func TestInternalTokenMismatchFailsClosed(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	// A wrong internal token must NOT fall through to the bearer path even when
	// a valid bearer is also present.
	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs(
		InternalTokenMetadataKey, "wrong-secret",
		"authorization", "Bearer "+sign(t, key, userClaims("org-a", "user-a")),
	))
	_, err := svc.Start(ctx, &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision", RunId: "run-1",
		Input: mustStruct(t, map[string]any{"goal": "g"}),
	})
	assert.Equal(t, codes.Unauthenticated, codeOf(err))
	assert.Empty(t, starter.starts)
}

// --- tenant containment ----------------------------------------------------

func TestCrossOrgStartIsDenied(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			OrgId:        "org-victim",
			RunId:        "run-1",
			Input:        mustStruct(t, map[string]any{"goal": "g"}),
		})
	assert.Equal(t, codes.PermissionDenied, codeOf(err))
	assert.Empty(t, starter.starts, "a cross-org start must never reach Temporal")
}

func TestInputTenancyIsOverwrittenByTheVerifiedCaller(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			RunId:        "run-1",
			// Hand-crafted body tenancy: must be ignored, not honoured.
			Input: mustStruct(t, map[string]any{
				"goal":    "g",
				"org_id":  "org-victim",
				"user_id": "someone-else",
				"run_id":  "run-hijack",
			}),
		})
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	in, ok := starter.starts[0].arg.(workflows.InteractiveRunInput)
	require.True(t, ok)
	assert.Equal(t, "org-a", in.OrgID)
	assert.Equal(t, "user-a", in.UserID)
	assert.Equal(t, "run-1", in.RunID)
	assert.Equal(t, "run-1", in.ThreadID)
}

func TestInternalCallerIsBoundToTheOrgAllowlist(t *testing.T) {
	auth, _ := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	base := func(org string) *mpv1.StartWorkflowRequest {
		return &mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			OrgId:        org,
			RunId:        "run-1",
			Input:        mustStruct(t, map[string]any{"goal": "g"}),
		}
	}

	_, err := svc.Start(internalCtx(internalTok), base("org-not-listed"))
	assert.Equal(t, codes.PermissionDenied, codeOf(err))

	_, err = svc.Start(internalCtx(internalTok), base(""))
	assert.Equal(t, codes.InvalidArgument, codeOf(err))

	_, err = svc.Start(internalCtx(internalTok), base("org-allowed"))
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	in := starter.starts[0].arg.(workflows.InteractiveRunInput)
	assert.Equal(t, "org-allowed", in.OrgID)
	assert.Empty(t, in.UserID, "a workload run is org-scoped, never viewer-scoped")
}

func TestServiceCallerCannotImpersonateAUser(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	token := sign(t, key, serviceClaims("org-a", []string{ScopeWorkflowStart}, false))
	_, err := svc.Start(bearerCtx(token), &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision",
		RunId:        "run-1",
		UserId:       "victim-user",
		Input:        mustStruct(t, map[string]any{"goal": "g"}),
	})
	assert.Equal(t, codes.PermissionDenied, codeOf(err))
	assert.Empty(t, starter.starts)
}

func TestForgedIdentityMetadataIsRejected(t *testing.T) {
	auth, key := testAuth(t)
	svc := NewWorkflowStartService(auth, &fakeStarter{}, testQueue)

	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs(
		"authorization", "Bearer "+sign(t, key, userClaims("org-a", "user-a")),
		"x-org-id", "org-victim",
	))
	_, err := svc.Start(ctx, &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision", RunId: "run-1",
		Input: mustStruct(t, map[string]any{"goal": "g"}),
	})
	assert.Equal(t, codes.PermissionDenied, codeOf(err))
}

// --- policy ----------------------------------------------------------------

func TestMaintenanceWorkflowsRejectInteractiveUsers(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType:   "MemoryConsolidationWorkflow",
			IdempotencyKey: "nightly-2026-07-31",
		})
	assert.Equal(t, codes.PermissionDenied, codeOf(err))
	assert.Empty(t, starter.starts)
}

func TestMaintenanceWorkflowNeedsTheStartScope(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	req := &mpv1.StartWorkflowRequest{
		WorkflowType:   "MemoryConsolidationWorkflow",
		IdempotencyKey: "nightly-2026-07-31",
	}

	_, err := svc.Start(bearerCtx(sign(t, key, serviceClaims("org-a", []string{"capability:read"}, false))), req)
	assert.Equal(t, codes.PermissionDenied, codeOf(err))

	_, err = svc.Start(bearerCtx(sign(t, key, serviceClaims("org-a", []string{ScopeWorkflowStart}, false))), req)
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	in := starter.starts[0].arg.(workflows.MemoryConsolidationInput)
	assert.Equal(t, "org-a", in.OrgID)
}

func TestRegistryWideWorkflowsNeedTheGlobalScopeAndRefuseTheInternalSecret(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	req := func() *mpv1.StartWorkflowRequest {
		return &mpv1.StartWorkflowRequest{
			WorkflowType:   "SkillPromotionWorkflow",
			IdempotencyKey: "promote-skill-1",
			Input:          mustStruct(t, map[string]any{"skill_id": "s1", "to_scope": "org"}),
		}
	}

	// A shared secret must never trigger a mutation reaching beyond one tenant.
	r := req()
	r.OrgId = "org-allowed"
	_, err := svc.Start(internalCtx(internalTok), r)
	assert.Equal(t, codes.PermissionDenied, codeOf(err))

	// Ordinary start scope is not enough.
	_, err = svc.Start(bearerCtx(sign(t, key, serviceClaims("org-a", []string{ScopeWorkflowStart}, false))), req())
	assert.Equal(t, codes.PermissionDenied, codeOf(err))

	_, err = svc.Start(bearerCtx(sign(t, key,
		serviceClaims("org-a", []string{ScopeWorkflowStart, ScopeWorkflowStartGlobal}, false))), req())
	require.NoError(t, err)
	assert.Len(t, starter.starts, 1)
}

// --- retention -------------------------------------------------------------

func TestMissingRetentionPostureIsDenied(t *testing.T) {
	auth, key := testAuth(t)
	svc := NewWorkflowStartService(auth, &fakeStarter{}, testQueue)

	// No `zdr` claim at all: the posture is unknown, and unknown is not
	// permission to persist.
	token := sign(t, key, testClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user"})
	_, err := svc.Start(bearerCtx(token), &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision", RunId: "run-1",
		Input: mustStruct(t, map[string]any{"goal": "g"}),
	})
	assert.Equal(t, codes.PermissionDenied, codeOf(err))
}

func TestZDRCallerCannotStartContentPersistingWorkflows(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	zdrService := sign(t, key, serviceClaims("org-a", []string{ScopeWorkflowStart}, true))
	_, err := svc.Start(bearerCtx(zdrService), &mpv1.StartWorkflowRequest{
		WorkflowType:   "MemoryConsolidationWorkflow",
		IdempotencyKey: "nightly",
	})
	assert.Equal(t, codes.PermissionDenied, codeOf(err))
	assert.Empty(t, starter.starts)
}

func TestZDRPostureIsPropagatedIntoTheWorkflowInput(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	zdrUser := sign(t, key, testClaims{
		OrgID: "org-a", UserID: "user-a", PrincipalType: "user", ZDR: boolPtr(true),
	})
	_, err := svc.Start(bearerCtx(zdrUser), &mpv1.StartWorkflowRequest{
		WorkflowType: "EvaluatorOptimizerWorkflow",
		RunId:        "run-1",
		Input:        mustStruct(t, map[string]any{"task": "write a haiku", "zdr": false}),
	})
	// `zdr` is not part of the wire contract, so supplying it is a hard error
	// rather than a silent override.
	require.Error(t, err)
	assert.Equal(t, codes.InvalidArgument, codeOf(err))

	_, err = svc.Start(bearerCtx(zdrUser), &mpv1.StartWorkflowRequest{
		WorkflowType: "EvaluatorOptimizerWorkflow",
		RunId:        "run-1",
		Input:        mustStruct(t, map[string]any{"task": "write a haiku"}),
	})
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	in := starter.starts[0].arg.(activities.EvalOptimizerInput)
	assert.True(t, in.ZDR, "the signed posture must reach the durable input")
	assert.True(t, in.RetentionAttested,
		"the presence of the claim must travel too, or zdr:false is ambiguous downstream")
	assert.Equal(t, activities.RetentionZeroData, in.Retention(),
		"the lifecycle-envelope posture must resolve from the signed claim")
	assert.Equal(t, "org-a", in.OrgID)
	assert.Equal(t, "run-1", in.RunID)
}

// TestRetentionPostureReachesTheInteractiveRunInput closes the leg that made the
// whole chain inert: the caller's signed posture has always been verified at
// start, but nothing carried it into the run's lifecycle envelope, so
// RUN_COMPLETED declared no `zdr` and every downstream retention gate refused.
func TestRetentionPostureReachesTheInteractiveRunInput(t *testing.T) {
	for _, tc := range []struct {
		name  string
		claim *bool
		want  activities.Retention
	}{
		{name: "attested non-ZDR caller", claim: boolPtr(false), want: activities.RetentionDurable},
		{name: "attested ZDR caller", claim: boolPtr(true), want: activities.RetentionZeroData},
	} {
		t.Run(tc.name, func(t *testing.T) {
			auth, key := testAuth(t)
			starter := &fakeStarter{}
			svc := NewWorkflowStartService(auth, starter, testQueue)

			token := sign(t, key, testClaims{
				OrgID: "org-a", UserID: "user-a", PrincipalType: "user", ZDR: tc.claim,
			})
			_, err := svc.Start(bearerCtx(token), &mpv1.StartWorkflowRequest{
				WorkflowType: "InteractiveRunSupervision", RunId: "run-1",
				Input: mustStruct(t, map[string]any{"goal": "g"}),
			})
			require.NoError(t, err)
			require.Len(t, starter.starts, 1)
			in := starter.starts[0].arg.(workflows.InteractiveRunInput)
			assert.Equal(t, tc.want, in.Retention)
		})
	}
}

// TestClientCannotForgeTheRetentionPosture — Retention is server-owned like
// OrgID/UserID: a ZDR caller that puts `retention: durable` in the request body
// must still get a ZDR run, or the flag would be a self-service opt-out from its
// own privacy guarantee.
func TestClientCannotForgeTheRetentionPosture(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	zdrUser := sign(t, key, testClaims{
		OrgID: "org-a", UserID: "user-a", PrincipalType: "user", ZDR: boolPtr(true),
	})
	_, err := svc.Start(bearerCtx(zdrUser), &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision", RunId: "run-1",
		Input: mustStruct(t, map[string]any{"goal": "g", "retention": string(activities.RetentionDurable)}),
	})
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	in := starter.starts[0].arg.(workflows.InteractiveRunInput)
	assert.Equal(t, activities.RetentionZeroData, in.Retention,
		"the signed posture must overwrite anything the client sent")
}

// TestInternalStarterCarriesUnspecifiedRetention documents the one start path
// with no signed posture at all. It must NOT be defaulted to durable: the
// shared-secret caller has made no retention assertion, so the run's envelope
// declares nothing and every content-persisting consumer downstream refuses.
// The cost is real — such runs teach the skill-learning loop nothing — and it is
// the correct trade: an inert loop is recoverable, a leaked skill is not.
func TestInternalStarterCarriesUnspecifiedRetention(t *testing.T) {
	auth, _ := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	_, err := svc.Start(internalCtx(internalTok), &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision",
		OrgId:        "org-allowed",
		RunId:        "run-1",
		Input:        mustStruct(t, map[string]any{"goal": "g"}),
	})
	require.NoError(t, err)
	require.Len(t, starter.starts, 1)
	in := starter.starts[0].arg.(workflows.InteractiveRunInput)
	assert.Equal(t, activities.RetentionUnspecified, in.Retention)
	assert.False(t, in.Retention.AllowsContent())
}

// TestEveryJWTStartHasAnAttestedPosture pins why RetentionUnspecified is rare in
// practice: authorizeRetention already refuses a JWT caller with no explicit
// posture, so a bearer-started run always resolves to ZeroData or Durable.
func TestEveryJWTStartHasAnAttestedPosture(t *testing.T) {
	for _, claim := range []*bool{boolPtr(false), boolPtr(true)} {
		caller := Caller{RetentionPolicyPresent: true, ZDR: *claim}
		got := Tenancy{ZDR: caller.ZDR, RetentionAttested: caller.RetentionPolicyPresent}.Retention()
		assert.NotEqual(t, activities.RetentionUnspecified, got)
	}
	unattested := Tenancy{ZDR: false, RetentionAttested: false}
	assert.Equal(t, activities.RetentionUnspecified, unattested.Retention(),
		"absent must never render as durable")
}

// --- input validation ------------------------------------------------------

func TestRunIDMustBeASingleNATSToken(t *testing.T) {
	// mp.v1.run.<run_id>.event is consumed via mp.v1.run.*.event, where `*`
	// matches ONE token — a dotted run id would publish RUN_COMPLETED where
	// nothing is listening and silently break the learning loop.
	for _, bad := range []string{"", " ", "run.1", "run 1", "run*", "run>", "\trun"} {
		assert.Error(t, ValidateRunID(bad), "run id %q must be rejected", bad)
	}
	assert.NoError(t, ValidateRunID("task_9f0c-1a2b"))
}

func TestRunScopedWorkflowRequiresARunID(t *testing.T) {
	auth, key := testAuth(t)
	svc := NewWorkflowStartService(auth, &fakeStarter{}, testQueue)
	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			Input:        mustStruct(t, map[string]any{"goal": "g"}),
		})
	assert.Equal(t, codes.InvalidArgument, codeOf(err))
}

func TestUnknownInputFieldsAreRejected(t *testing.T) {
	auth, key := testAuth(t)
	svc := NewWorkflowStartService(auth, &fakeStarter{}, testQueue)
	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			RunId:        "run-1",
			Input:        mustStruct(t, map[string]any{"goal": "g", "goaal": "typo"}),
		})
	assert.Equal(t, codes.InvalidArgument, codeOf(err))
}

func TestEmptyGoalIsRejected(t *testing.T) {
	auth, key := testAuth(t)
	svc := NewWorkflowStartService(auth, &fakeStarter{}, testQueue)
	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			RunId:        "run-1",
			Input:        mustStruct(t, map[string]any{"goal": "   "}),
		})
	assert.Equal(t, codes.InvalidArgument, codeOf(err))
}

// --- workflow id -----------------------------------------------------------

func TestWorkflowIDIsDeterministicAndTenantSeparated(t *testing.T) {
	a := WorkflowID("InteractiveRunSupervision", "org-a", "run-1")
	assert.Equal(t, a, WorkflowID("InteractiveRunSupervision", "org-a", "run-1"),
		"same inputs must yield the same id so a retry cannot double-start")
	assert.NotEqual(t, a, WorkflowID("InteractiveRunSupervision", "org-b", "run-1"),
		"two orgs must never collide on one workflow id")
	assert.NotEqual(t, a, WorkflowID("DeepTaskWorkflow", "org-a", "run-1"))
	assert.Contains(t, a, "InteractiveRunSupervision")
	assert.Contains(t, a, "run-1")
	assert.NotContains(t, a, "org-a", "the raw org id is hashed, not interpolated")
}

func TestRetriedStartReusesTheSameWorkflowID(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	req := &mpv1.StartWorkflowRequest{
		WorkflowType: "InteractiveRunSupervision",
		RunId:        "run-retry",
		Input:        mustStruct(t, map[string]any{"goal": "g"}),
	}
	ctx := bearerCtx(sign(t, key, userClaims("org-a", "user-a")))
	first, err := svc.Start(ctx, req)
	require.NoError(t, err)
	second, err := svc.Start(ctx, req)
	require.NoError(t, err)
	assert.Equal(t, first.GetWorkflowId(), second.GetWorkflowId())
	assert.Equal(t, "InteractiveRunSupervision", second.GetWorkflowType())
}

func TestIdempotencyKeyReplacesTheRunAnchor(t *testing.T) {
	auth, key := testAuth(t)
	starter := &fakeStarter{}
	svc := NewWorkflowStartService(auth, starter, testQueue)

	resp, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType:   "InteractiveRunSupervision",
			RunId:          "run-1",
			IdempotencyKey: "dedupe-me",
			Input:          mustStruct(t, map[string]any{"goal": "g"}),
		})
	require.NoError(t, err)
	assert.Contains(t, resp.GetWorkflowId(), "dedupe-me")
	assert.NotContains(t, resp.GetWorkflowId(), "run-1")
}

func TestStarterErrorSurfacesAsInternal(t *testing.T) {
	auth, key := testAuth(t)
	svc := NewWorkflowStartService(auth, &fakeStarter{err: errors.New("temporal down")}, testQueue)
	_, err := svc.Start(bearerCtx(sign(t, key, userClaims("org-a", "user-a"))),
		&mpv1.StartWorkflowRequest{
			WorkflowType: "InteractiveRunSupervision",
			RunId:        "run-1",
			Input:        mustStruct(t, map[string]any{"goal": "g"}),
		})
	assert.Equal(t, codes.Internal, codeOf(err))
}
