package orchestration

import (
	"context"
	"errors"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// stubClient captures the last request passed to each method and returns
// the configured response/error. Implements mpv1.OrchestrationCoreServiceClient.
type stubClient struct {
	lastReq any
	err     error

	listPlansResp                *mpv1.ListPlansResponse
	getPlanResp                  *mpv1.GetPlanResponse
	transitionPlanResp           *mpv1.TransitionPlanResponse
	listTodosResp                *mpv1.ListTodosResponse
	getTodoResp                  *mpv1.GetTodoResponse
	transitionTodoResp           *mpv1.TransitionTodoResponse
	listApprovalsResp            *mpv1.ListApprovalsResponse
	listPendingApprovalsResp     *mpv1.OrgPendingApprovalsResponse
	recordOrchestrationEventResp *mpv1.RecordOrchestrationEventResponse
	getApprovalResp              *mpv1.GetApprovalResponse
	createApprovalResp           *mpv1.CreateApprovalResponse
	decideApprovalResp           *mpv1.DecideApprovalResponse
	claimApprovalDeliveriesResp  *mpv1.ClaimApprovalDeliveriesResponse
	ackApprovalDeliveryResp      *mpv1.AcknowledgeApprovalDeliveryResponse
	getSubagentLineageResp       *mpv1.GetSubagentLineageResponse
	attachSubagentResp           *mpv1.AttachSubagentResponse
	getRunProofBundleResp        *mpv1.GetRunProofBundleResponse
}

func (s *stubClient) ListPlans(ctx context.Context, in *mpv1.ListPlansRequest, opts ...grpc.CallOption) (*mpv1.ListPlansResponse, error) {
	s.lastReq = in
	return s.listPlansResp, s.err
}
func (s *stubClient) GetPlan(ctx context.Context, in *mpv1.GetPlanRequest, opts ...grpc.CallOption) (*mpv1.GetPlanResponse, error) {
	s.lastReq = in
	return s.getPlanResp, s.err
}
func (s *stubClient) TransitionPlan(ctx context.Context, in *mpv1.TransitionPlanRequest, opts ...grpc.CallOption) (*mpv1.TransitionPlanResponse, error) {
	s.lastReq = in
	return s.transitionPlanResp, s.err
}
func (s *stubClient) ListTodos(ctx context.Context, in *mpv1.ListTodosRequest, opts ...grpc.CallOption) (*mpv1.ListTodosResponse, error) {
	s.lastReq = in
	return s.listTodosResp, s.err
}
func (s *stubClient) GetTodo(ctx context.Context, in *mpv1.GetTodoRequest, opts ...grpc.CallOption) (*mpv1.GetTodoResponse, error) {
	s.lastReq = in
	return s.getTodoResp, s.err
}
func (s *stubClient) TransitionTodo(ctx context.Context, in *mpv1.TransitionTodoRequest, opts ...grpc.CallOption) (*mpv1.TransitionTodoResponse, error) {
	s.lastReq = in
	return s.transitionTodoResp, s.err
}
func (s *stubClient) ListApprovals(ctx context.Context, in *mpv1.ListApprovalsRequest, opts ...grpc.CallOption) (*mpv1.ListApprovalsResponse, error) {
	s.lastReq = in
	return s.listApprovalsResp, s.err
}
func (s *stubClient) ListPendingApprovals(ctx context.Context, in *mpv1.OrgPendingApprovalsRequest, opts ...grpc.CallOption) (*mpv1.OrgPendingApprovalsResponse, error) {
	s.lastReq = in
	return s.listPendingApprovalsResp, s.err
}
func (s *stubClient) RecordOrchestrationEvent(ctx context.Context, in *mpv1.RecordOrchestrationEventRequest, opts ...grpc.CallOption) (*mpv1.RecordOrchestrationEventResponse, error) {
	s.lastReq = in
	return s.recordOrchestrationEventResp, s.err
}
func (s *stubClient) GetApproval(ctx context.Context, in *mpv1.GetApprovalRequest, opts ...grpc.CallOption) (*mpv1.GetApprovalResponse, error) {
	s.lastReq = in
	return s.getApprovalResp, s.err
}
func (s *stubClient) CreateApproval(ctx context.Context, in *mpv1.CreateApprovalRequest, opts ...grpc.CallOption) (*mpv1.CreateApprovalResponse, error) {
	s.lastReq = in
	return s.createApprovalResp, s.err
}
func (s *stubClient) DecideApproval(ctx context.Context, in *mpv1.DecideApprovalRequest, opts ...grpc.CallOption) (*mpv1.DecideApprovalResponse, error) {
	s.lastReq = in
	return s.decideApprovalResp, s.err
}
func (s *stubClient) ClaimApprovalDeliveries(ctx context.Context, in *mpv1.ClaimApprovalDeliveriesRequest, opts ...grpc.CallOption) (*mpv1.ClaimApprovalDeliveriesResponse, error) {
	s.lastReq = in
	return s.claimApprovalDeliveriesResp, s.err
}

// GetRunProofBundle is now proxied by Handlers (F-15 fix) and exercised by
// the tests below like every other unary RPC.
func (s *stubClient) GetRunProofBundle(ctx context.Context, in *mpv1.GetRunProofBundleRequest, opts ...grpc.CallOption) (*mpv1.GetRunProofBundleResponse, error) {
	s.lastReq = in
	return s.getRunProofBundleResp, s.err
}

// GetVerificationMetrics predates this stub's own completeness: the Go
// bindings were only just regenerated from a .proto contract that had
// already declared it (P1.5) - it does not yet reach a handler path, but a
// stub must satisfy the whole client interface or the package fails to build.
func (s *stubClient) GetVerificationMetrics(ctx context.Context, in *mpv1.GetVerificationMetricsRequest, opts ...grpc.CallOption) (*mpv1.GetVerificationMetricsResponse, error) {
	s.lastReq = in
	return nil, s.err
}

func (s *stubClient) GetApprovalContinuation(ctx context.Context, in *mpv1.GetApprovalContinuationRequest, opts ...grpc.CallOption) (*mpv1.GetApprovalContinuationResponse, error) {
	s.lastReq = in
	return nil, s.err
}
func (s *stubClient) RecordApprovalContinuationStarted(ctx context.Context, in *mpv1.RecordApprovalContinuationStartedRequest, opts ...grpc.CallOption) (*mpv1.RecordApprovalContinuationStartedResponse, error) {
	s.lastReq = in
	return nil, s.err
}
func (s *stubClient) RecordApprovalContinuationOutcome(ctx context.Context, in *mpv1.RecordApprovalContinuationOutcomeRequest, opts ...grpc.CallOption) (*mpv1.RecordApprovalContinuationOutcomeResponse, error) {
	s.lastReq = in
	return nil, s.err
}

func (s *stubClient) AcknowledgeApprovalDelivery(ctx context.Context, in *mpv1.AcknowledgeApprovalDeliveryRequest, opts ...grpc.CallOption) (*mpv1.AcknowledgeApprovalDeliveryResponse, error) {
	s.lastReq = in
	return s.ackApprovalDeliveryResp, s.err
}
func (s *stubClient) GetSubagentLineage(ctx context.Context, in *mpv1.GetSubagentLineageRequest, opts ...grpc.CallOption) (*mpv1.GetSubagentLineageResponse, error) {
	s.lastReq = in
	return s.getSubagentLineageResp, s.err
}
func (s *stubClient) AttachSubagent(ctx context.Context, in *mpv1.AttachSubagentRequest, opts ...grpc.CallOption) (*mpv1.AttachSubagentResponse, error) {
	s.lastReq = in
	return s.attachSubagentResp, s.err
}
func (s *stubClient) StreamRunEvents(ctx context.Context, in *mpv1.StreamRunEventsRequest, opts ...grpc.CallOption) (grpc.ServerStreamingClient[mpv1.OrchestrationEvent], error) {
	s.lastReq = in
	return nil, s.err
}

// TestHandlers_NilClient verifies every unary RPC returns codes.Unavailable
// when no upstream session-core client is wired.
func TestHandlers_NilClient(t *testing.T) {
	h := NewHandlers(nil, nil)
	ctx := context.Background()

	type call struct {
		name string
		fn   func() error
	}
	calls := []call{
		{"ListPlans", func() error { _, e := h.ListPlans(ctx, &mpv1.ListPlansRequest{}); return e }},
		{"GetPlan", func() error { _, e := h.GetPlan(ctx, &mpv1.GetPlanRequest{}); return e }},
		{"TransitionPlan", func() error { _, e := h.TransitionPlan(ctx, &mpv1.TransitionPlanRequest{}); return e }},
		{"ListTodos", func() error { _, e := h.ListTodos(ctx, &mpv1.ListTodosRequest{}); return e }},
		{"GetTodo", func() error { _, e := h.GetTodo(ctx, &mpv1.GetTodoRequest{}); return e }},
		{"TransitionTodo", func() error { _, e := h.TransitionTodo(ctx, &mpv1.TransitionTodoRequest{}); return e }},
		{"ListApprovals", func() error { _, e := h.ListApprovals(ctx, &mpv1.ListApprovalsRequest{}); return e }},
		{"GetApproval", func() error { _, e := h.GetApproval(ctx, &mpv1.GetApprovalRequest{}); return e }},
		{"DecideApproval", func() error { _, e := h.DecideApproval(ctx, &mpv1.DecideApprovalRequest{}); return e }},
		{"GetSubagentLineage", func() error { _, e := h.GetSubagentLineage(ctx, &mpv1.GetSubagentLineageRequest{}); return e }},
		{"AttachSubagent", func() error { _, e := h.AttachSubagent(ctx, &mpv1.AttachSubagentRequest{}); return e }},
		{"GetRunProofBundle", func() error { _, e := h.GetRunProofBundle(ctx, &mpv1.GetRunProofBundleRequest{}); return e }},
	}
	for _, c := range calls {
		t.Run(c.name, func(t *testing.T) {
			err := c.fn()
			require.Error(t, err)
			st, ok := status.FromError(err)
			require.True(t, ok, "expected gRPC status error")
			assert.Equal(t, codes.Unavailable, st.Code())
			assert.Contains(t, st.Message(), "session-core client not configured")
		})
	}
}

// TestHandlers_ProxyToClient verifies each unary RPC forwards the request to
// the upstream client and returns its response.
func TestHandlers_ProxyToClient(t *testing.T) {
	stub := &stubClient{
		listPlansResp:          &mpv1.ListPlansResponse{},
		getPlanResp:            &mpv1.GetPlanResponse{},
		transitionPlanResp:     &mpv1.TransitionPlanResponse{},
		listTodosResp:          &mpv1.ListTodosResponse{},
		getTodoResp:            &mpv1.GetTodoResponse{},
		transitionTodoResp:     &mpv1.TransitionTodoResponse{},
		listApprovalsResp:      &mpv1.ListApprovalsResponse{},
		getApprovalResp:        &mpv1.GetApprovalResponse{},
		decideApprovalResp:     &mpv1.DecideApprovalResponse{},
		getSubagentLineageResp: &mpv1.GetSubagentLineageResponse{},
		attachSubagentResp:     &mpv1.AttachSubagentResponse{},
		getRunProofBundleResp:  &mpv1.GetRunProofBundleResponse{Bundle: &mpv1.RunProofBundle{RunId: "run_01"}},
	}
	h := NewHandlers(nil, stub)
	ctx := context.Background()

	listPlansReq := &mpv1.ListPlansRequest{}
	resp1, err := h.ListPlans(ctx, listPlansReq)
	require.NoError(t, err)
	assert.Same(t, stub.listPlansResp, resp1)
	assert.Same(t, listPlansReq, stub.lastReq)

	getPlanReq := &mpv1.GetPlanRequest{}
	resp2, err := h.GetPlan(ctx, getPlanReq)
	require.NoError(t, err)
	assert.Same(t, stub.getPlanResp, resp2)
	assert.Same(t, getPlanReq, stub.lastReq)

	transitionPlanReq := &mpv1.TransitionPlanRequest{}
	resp3, err := h.TransitionPlan(ctx, transitionPlanReq)
	require.NoError(t, err)
	assert.Same(t, stub.transitionPlanResp, resp3)
	assert.Same(t, transitionPlanReq, stub.lastReq)

	listTodosReq := &mpv1.ListTodosRequest{}
	resp4, err := h.ListTodos(ctx, listTodosReq)
	require.NoError(t, err)
	assert.Same(t, stub.listTodosResp, resp4)
	assert.Same(t, listTodosReq, stub.lastReq)

	getTodoReq := &mpv1.GetTodoRequest{}
	resp5, err := h.GetTodo(ctx, getTodoReq)
	require.NoError(t, err)
	assert.Same(t, stub.getTodoResp, resp5)
	assert.Same(t, getTodoReq, stub.lastReq)

	transitionTodoReq := &mpv1.TransitionTodoRequest{}
	resp6, err := h.TransitionTodo(ctx, transitionTodoReq)
	require.NoError(t, err)
	assert.Same(t, stub.transitionTodoResp, resp6)
	assert.Same(t, transitionTodoReq, stub.lastReq)

	listApprovalsReq := &mpv1.ListApprovalsRequest{}
	resp7, err := h.ListApprovals(ctx, listApprovalsReq)
	require.NoError(t, err)
	assert.Same(t, stub.listApprovalsResp, resp7)
	assert.Same(t, listApprovalsReq, stub.lastReq)

	getApprovalReq := &mpv1.GetApprovalRequest{}
	resp8, err := h.GetApproval(ctx, getApprovalReq)
	require.NoError(t, err)
	assert.Same(t, stub.getApprovalResp, resp8)
	assert.Same(t, getApprovalReq, stub.lastReq)

	decideApprovalReq := &mpv1.DecideApprovalRequest{}
	resp9, err := h.DecideApproval(ctx, decideApprovalReq)
	require.NoError(t, err)
	assert.Same(t, stub.decideApprovalResp, resp9)
	assert.Same(t, decideApprovalReq, stub.lastReq)

	getSubagentLineageReq := &mpv1.GetSubagentLineageRequest{}
	resp10, err := h.GetSubagentLineage(ctx, getSubagentLineageReq)
	require.NoError(t, err)
	assert.Same(t, stub.getSubagentLineageResp, resp10)
	assert.Same(t, getSubagentLineageReq, stub.lastReq)

	attachSubagentReq := &mpv1.AttachSubagentRequest{}
	resp11, err := h.AttachSubagent(ctx, attachSubagentReq)
	require.NoError(t, err)
	assert.Same(t, stub.attachSubagentResp, resp11)
	assert.Same(t, attachSubagentReq, stub.lastReq)

	getRunProofBundleReq := &mpv1.GetRunProofBundleRequest{RunId: "run_01", OrgId: "org_01"}
	resp12, err := h.GetRunProofBundle(ctx, getRunProofBundleReq)
	require.NoError(t, err)
	assert.Same(t, stub.getRunProofBundleResp, resp12)
	assert.Same(t, getRunProofBundleReq, stub.lastReq)
}

// TestHandlers_PropagatesUpstreamError verifies upstream errors surface unchanged.
func TestHandlers_PropagatesUpstreamError(t *testing.T) {
	wantErr := errors.New("upstream boom")
	stub := &stubClient{err: wantErr}
	h := NewHandlers(nil, stub)

	_, err := h.ListPlans(context.Background(), &mpv1.ListPlansRequest{})
	require.ErrorIs(t, err, wantErr)
}

// TestHandlers_StreamRunEvents_NilClient verifies streaming honors nil-client guard.
func TestHandlers_StreamRunEvents_NilClient(t *testing.T) {
	h := NewHandlers(nil, nil)
	err := h.StreamRunEvents(&mpv1.StreamRunEventsRequest{}, nil)
	require.Error(t, err)
	st, ok := status.FromError(err)
	require.True(t, ok)
	assert.Equal(t, codes.Unavailable, st.Code())
}

// TestHandlers_Service verifies the Service accessor returns the wired Service.
func TestHandlers_Service(t *testing.T) {
	h := NewHandlers(nil, nil)
	assert.Nil(t, h.Service())
}
