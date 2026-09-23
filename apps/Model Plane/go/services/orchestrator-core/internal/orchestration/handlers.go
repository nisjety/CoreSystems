package orchestration

import (
	"context"
	"io"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Handlers implements mpv1.OrchestrationCoreServiceServer by proxying RPCs
// to the Rust session-core backend. The local Service is retained so future
// transition flows can publish NATS events alongside the upstream call.
type Handlers struct {
	mpv1.UnimplementedOrchestrationCoreServiceServer
	svc    *Service
	client mpv1.OrchestrationCoreServiceClient
}

// NewHandlers builds a gRPC handler set bound to the given Service and
// upstream session-core client. The client may be nil in tests, in which
// case all RPCs return codes.Unavailable.
func NewHandlers(svc *Service, client mpv1.OrchestrationCoreServiceClient) *Handlers {
	return &Handlers{svc: svc, client: client}
}

// Service returns the underlying publishing Service. Useful for tests and
// future wiring layers that need direct access without going through gRPC.
func (h *Handlers) Service() *Service { return h.svc }

func (h *Handlers) requireClient() error {
	if h.client == nil {
		return status.Error(codes.Unavailable, "session-core client not configured")
	}
	return nil
}

func (h *Handlers) ListPlans(ctx context.Context, req *mpv1.ListPlansRequest) (*mpv1.ListPlansResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.ListPlans(ctx, req)
}

func (h *Handlers) GetPlan(ctx context.Context, req *mpv1.GetPlanRequest) (*mpv1.GetPlanResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.GetPlan(ctx, req)
}

func (h *Handlers) TransitionPlan(ctx context.Context, req *mpv1.TransitionPlanRequest) (*mpv1.TransitionPlanResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.TransitionPlan(ctx, req)
}

func (h *Handlers) ListTodos(ctx context.Context, req *mpv1.ListTodosRequest) (*mpv1.ListTodosResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.ListTodos(ctx, req)
}

func (h *Handlers) GetTodo(ctx context.Context, req *mpv1.GetTodoRequest) (*mpv1.GetTodoResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.GetTodo(ctx, req)
}

func (h *Handlers) TransitionTodo(ctx context.Context, req *mpv1.TransitionTodoRequest) (*mpv1.TransitionTodoResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.TransitionTodo(ctx, req)
}

func (h *Handlers) ListApprovals(ctx context.Context, req *mpv1.ListApprovalsRequest) (*mpv1.ListApprovalsResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.ListApprovals(ctx, req)
}

func (h *Handlers) GetApproval(ctx context.Context, req *mpv1.GetApprovalRequest) (*mpv1.GetApprovalResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.GetApproval(ctx, req)
}

func (h *Handlers) DecideApproval(ctx context.Context, req *mpv1.DecideApprovalRequest) (*mpv1.DecideApprovalResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.DecideApproval(ctx, req)
}

// CreateApproval / ListPendingApprovals / RecordOrchestrationEvent are the
// HITL-durability RPCs. They were previously left to the embedded
// UnimplementedOrchestrationCoreServiceServer (returning codes.Unimplemented),
// which broke minting/listing approvals and recording run events through the
// gateway. Proxy them to session-core like the other RPCs; the dial-site
// interceptor forwards the caller's credential.
func (h *Handlers) CreateApproval(ctx context.Context, req *mpv1.CreateApprovalRequest) (*mpv1.CreateApprovalResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.CreateApproval(ctx, req)
}

func (h *Handlers) ListPendingApprovals(ctx context.Context, req *mpv1.OrgPendingApprovalsRequest) (*mpv1.OrgPendingApprovalsResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.ListPendingApprovals(ctx, req)
}

func (h *Handlers) RecordOrchestrationEvent(ctx context.Context, req *mpv1.RecordOrchestrationEventRequest) (*mpv1.RecordOrchestrationEventResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.RecordOrchestrationEvent(ctx, req)
}

func (h *Handlers) GetSubagentLineage(ctx context.Context, req *mpv1.GetSubagentLineageRequest) (*mpv1.GetSubagentLineageResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.GetSubagentLineage(ctx, req)
}

func (h *Handlers) AttachSubagent(ctx context.Context, req *mpv1.AttachSubagentRequest) (*mpv1.AttachSubagentResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.AttachSubagent(ctx, req)
}

// GetRunProofBundle assembles the Verevon Proof Bundle for one run (F-15,
// CHAT_PARITY_AUDIT_2026-09-15.md §3.10/§3.11). Like CreateApproval /
// ListPendingApprovals / RecordOrchestrationEvent above, this RPC was left to
// the embedded UnimplementedOrchestrationCoreServiceServer — every call
// answered codes.Unimplemented, which model-gateway's grpc_status_to_http
// catch-all turns into a bare 500 with no diagnostic, so the SPA's Trace/
// receipt panel failed on every run even though session-core has fully
// implemented get_run_proof_bundle (orchestration_grpc.rs) since the proof
// bundle's .proto landed. Proxy it like every other read RPC here; session-
// core is the sole owner of the approval/continuation-evidence tables the
// bundle is assembled from.
func (h *Handlers) GetRunProofBundle(ctx context.Context, req *mpv1.GetRunProofBundleRequest) (*mpv1.GetRunProofBundleResponse, error) {
	if err := h.requireClient(); err != nil {
		return nil, err
	}
	return h.client.GetRunProofBundle(ctx, req)
}

func (h *Handlers) StreamRunEvents(req *mpv1.StreamRunEventsRequest, stream grpc.ServerStreamingServer[mpv1.OrchestrationEvent]) error {
	if err := h.requireClient(); err != nil {
		return err
	}
	upstream, err := h.client.StreamRunEvents(stream.Context(), req)
	if err != nil {
		return err
	}
	for {
		ev, err := upstream.Recv()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
		if err := stream.Send(ev); err != nil {
			return err
		}
	}
}
