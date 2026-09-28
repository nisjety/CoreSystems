package server

import (
	"context"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/process"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// S4.2 background process registry handlers (design doc §5).
//
// The four WRITE RPCs are service-principal only, permanently: the "caller"
// is always execution-core's own host reporting what an OS process it owns
// actually did, and a user bearer has no business asserting any of it.
//
// Two of the three reads — ListProcesses and ReadProcessOutput — admit a human
// as of S4.2 step 6, but only behind a Control-signed model.thread.read
// decision addressed to this service, verified here the way session-core
// verifies its own for ListRuns. The decision is not merely an admission
// ticket: it carries the recipient-audience revision every returned row is
// filtered against, so a member cannot read work recorded under an audience
// their current decision does not cover.
//
// GetProcess stays service-only, and that is a decision rather than an
// oversight. It resolves one process by id, which is what execution-core needs
// to check a model-named id against its own Space before reading it. The human
// path pages a Space's list and reads output; nothing in it resolves a bare
// id, so admitting a human here would add an authority path with no caller —
// and the one shape most useful for probing which ids exist.

// requireProcessHost admits only a service principal. Used by the four RPCs
// a host calls about its own processes.
func requireProcessHost(principal authctx.Principal) error {
	if principal.PrincipalType == "user" {
		return status.Error(codes.PermissionDenied,
			"the process registry is written only by an execution host's service identity")
	}
	return nil
}

// requireProcessHostRead admits only a service principal. Used by GetProcess,
// which has no human caller — see the package comment.
func requireProcessHostRead(principal authctx.Principal) error {
	if principal.PrincipalType == "user" {
		return status.Error(codes.FailedPrecondition,
			"resolving a single process by id is an execution host's operation; a human reads a Space's processes through ListProcesses")
	}
	return nil
}

// resolveReadCeiling decides what a caller may see.
//
// A service principal reads unbounded, because it is not a disclosure
// recipient: execution-core is the thing that PRODUCED the record, and it
// applies its own Space check before showing anything to a model
// (execution-core's process_tools). A human reads under the audience ceiling
// carried by their own decision, and only with one.
//
// A user principal that supplies no decision is refused rather than silently
// downgraded to an empty result — "you did not bring authority" and "there is
// nothing here" are different answers and a caller acts differently on each.
func (s *Server) resolveReadCeiling(
	principal authctx.Principal, spaceID, decisionRef, decisionToken string,
) (*process.AudienceCeiling, error) {
	if principal.PrincipalType != "user" {
		return nil, nil
	}
	if s.spaceReadVerify == nil {
		return nil, status.Error(codes.FailedPrecondition, "Space read decision verification is not configured")
	}
	verified, err := s.spaceReadVerify(decisionToken, authz.SpaceReadExpectation{
		OrgID:       principal.OrganizationID,
		SpaceRef:    spaceID,
		SubjectID:   principal.ActorID,
		DecisionRef: decisionRef,
		Now:         time.Now().UTC(),
	})
	if err != nil {
		processDecision(context.Background(), "read_decision_denied")
		return nil, status.Error(codes.PermissionDenied, err.Error())
	}
	return &process.AudienceCeiling{RecipientAudienceRevision: verified.RecipientAudienceRevision}, nil
}

// processStore returns the configured registry, or an error explaining that
// this instance has none. A Server built without one (cmd/main.go's
// ephemeral-development mode) refuses rather than serving against state that
// disappears on restart.
func (s *Server) processStore() (ProcessStore, error) {
	if s.processes == nil {
		return nil, status.Error(codes.FailedPrecondition, "the background process registry is not configured")
	}
	return s.processes, nil
}

func processDecision(ctx context.Context, outcome string) {
	telemetry.ProcessDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", outcome)))
}

// RegisterProcess reserves a process row before the host spawns anything.
func (s *Server) RegisterProcess(ctx context.Context, req *RegisterProcessRequest) (*RegisterProcessResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "RegisterProcess")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if err := requireProcessHost(principal); err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	if req.GetProcessId() == "" {
		return nil, status.Error(codes.InvalidArgument, "process_id is required")
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	if req.GetCommand() == nil {
		return nil, status.Error(codes.InvalidArgument, "command is required")
	}
	p, err := store.Register(ctx, process.RegisterRequest{
		ID: req.GetProcessId(),
		// Never from the request: the tenant is whatever the verified
		// principal says it is.
		OrgID:     principal.OrganizationID,
		LeaseID:   req.GetLeaseId(),
		BackendID: req.GetBackendId(),
		HostEpoch: req.GetHostEpoch(),
		RunID:     req.GetRunId(),
		StepID:    req.GetStepId(),
		SubjectID: req.GetSubjectId(),
		Command: process.Command{
			Program: req.GetCommand().GetProgram(),
			Args:    req.GetCommand().GetArgs(),
		},
		CommandDigest: req.GetCommandDigest(),
		TTLSeconds:    req.GetTtlSeconds(),
	})
	if err != nil {
		processDecision(ctx, processOutcome(err))
		return nil, mapErr(err)
	}
	processDecision(ctx, "registered")
	return &RegisterProcessResponse{Process: processToProto(p)}, nil
}

// UpdateProcessState records a host-driven lifecycle transition.
func (s *Server) UpdateProcessState(ctx context.Context, req *UpdateProcessStateRequest) (*UpdateProcessStateResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "UpdateProcessState")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if err := requireProcessHost(principal); err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	fence, err := processFence(principal, req.GetProcessId(), req.GetBackendId(), req.GetHostEpoch())
	if err != nil {
		return nil, err
	}

	switch transition := req.GetTransition().(type) {
	case *mpv1.UpdateProcessStateRequest_Started:
		if err := store.MarkStarted(ctx, fence); err != nil {
			processDecision(ctx, processOutcome(err))
			return nil, mapErr(err)
		}
		processDecision(ctx, "started")
	case *mpv1.UpdateProcessStateRequest_Signal:
		signal, err := processSignalFromProto(transition.Signal.GetSignal())
		if err != nil {
			return nil, err
		}
		if err := store.RequestSignal(ctx, fence, signal); err != nil {
			processDecision(ctx, processOutcome(err))
			return nil, mapErr(err)
		}
		processDecision(ctx, "signal_requested")
	case *mpv1.UpdateProcessStateRequest_Exited:
		state, err := processStateFromProto(transition.Exited.GetState())
		if err != nil {
			return nil, err
		}
		if !state.IsTerminal() {
			return nil, status.Error(codes.InvalidArgument, "an exit must report a terminal state")
		}
		var exitCode *int32
		if transition.Exited.ExitCode != nil {
			code := transition.Exited.GetExitCode()
			exitCode = &code
		}
		if err := store.MarkEnded(ctx, fence, state, exitCode, transition.Exited.GetEndReason(), transition.Exited.GetCleanupDone()); err != nil {
			processDecision(ctx, processOutcome(err))
			return nil, mapErr(err)
		}
		processDecision(ctx, "ended")
	default:
		return nil, status.Error(codes.InvalidArgument, "a transition is required")
	}
	return &UpdateProcessStateResponse{}, nil
}

// AppendProcessOutput records output and refreshes the heartbeat.
func (s *Server) AppendProcessOutput(ctx context.Context, req *AppendProcessOutputRequest) (*AppendProcessOutputResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "AppendProcessOutput")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if err := requireProcessHost(principal); err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	fence, err := processFence(principal, req.GetProcessId(), req.GetBackendId(), req.GetHostEpoch())
	if err != nil {
		return nil, err
	}
	chunks := make([]process.Chunk, 0, len(req.GetChunks()))
	for _, c := range req.GetChunks() {
		stream, err := processStreamFromProto(c.GetStream())
		if err != nil {
			return nil, err
		}
		chunks = append(chunks, process.Chunk{
			Seq:             c.GetSeq(),
			Stream:          stream,
			Content:         c.GetContent(),
			EndsWithNewline: c.GetEndsWithNewline(),
			CapturedAt:      timestampOrNow(c.GetCapturedAt()),
		})
	}
	result, err := store.AppendOutput(ctx, fence, chunks, req.GetStdinBytesDelta())
	if err != nil {
		processDecision(ctx, processOutcome(err))
		return nil, mapErr(err)
	}
	processDecision(ctx, "appended")
	return &AppendProcessOutputResponse{
		NextSeq:         result.NextSeq,
		RetainedFromSeq: result.RetainedFromSeq,
		RetainedBytes:   result.RetainedBytes,
		DroppedBytes:    result.DroppedBytes,
	}, nil
}

// ReconcileProcesses declares a superseded host's live processes lost.
func (s *Server) ReconcileProcesses(ctx context.Context, req *ReconcileProcessesRequest) (*ReconcileProcessesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ReconcileProcesses")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if err := requireProcessHost(principal); err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	if req.GetBackendId() == "" || req.GetHostEpoch() == "" {
		return nil, status.Error(codes.InvalidArgument, "backend_id and host_epoch are required")
	}
	lost, err := store.Reconcile(ctx, req.GetBackendId(), req.GetHostEpoch())
	if err != nil {
		processDecision(ctx, processOutcome(err))
		return nil, mapErr(err)
	}
	processDecision(ctx, "reconciled")
	return &ReconcileProcessesResponse{LostCount: lost}, nil
}

// GetProcess reads one process's metadata within the caller's organization.
func (s *Server) GetProcess(ctx context.Context, req *GetProcessRequest) (*GetProcessResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetProcess")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if err := requireProcessHostRead(principal); err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	if req.GetProcessId() == "" {
		return nil, status.Error(codes.InvalidArgument, "process_id is required")
	}
	p, err := store.Get(ctx, principal.OrganizationID, req.GetProcessId())
	if err != nil {
		return nil, mapErr(err)
	}
	return &GetProcessResponse{Process: processToProto(p)}, nil
}

// ListProcesses pages a Space's processes, newest first.
func (s *Server) ListProcesses(ctx context.Context, req *ListProcessesRequest) (*ListProcessesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ListProcesses")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	if req.GetSpaceId() == "" {
		return nil, status.Error(codes.InvalidArgument, "space_id is required")
	}
	ceiling, err := s.resolveReadCeiling(principal, req.GetSpaceId(),
		req.GetSpaceReadDecisionRef(), req.GetSpaceReadDecisionToken())
	if err != nil {
		return nil, err
	}
	found, hasMore, err := store.List(ctx, principal.OrganizationID, req.GetSpaceId(),
		req.GetIncludeTerminal(), req.GetLimit(), req.GetAfterProcessId(), ceiling)
	if err != nil {
		return nil, mapErr(err)
	}
	out := make([]*Process, 0, len(found))
	for i := range found {
		out = append(out, processToProto(&found[i]))
	}
	return &ListProcessesResponse{Processes: out, HasMore: hasMore}, nil
}

// ReadProcessOutput returns the chunks after a cursor.
func (s *Server) ReadProcessOutput(ctx context.Context, req *ReadProcessOutputRequest) (*ReadProcessOutputResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ReadProcessOutput")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	store, err := s.processStore()
	if err != nil {
		return nil, err
	}
	if req.GetProcessId() == "" {
		return nil, status.Error(codes.InvalidArgument, "process_id is required")
	}
	// The Space comes from the REQUEST, and the store then refuses any process
	// whose own Space does not match what the decision covers. Reading the
	// Space off the process row first and verifying the decision against that
	// would let a caller present a decision for Space A and have it checked
	// against Space B — the verifier would compare the token to whatever the
	// row said and pass.
	ceiling, err := s.resolveReadCeiling(principal, req.GetSpaceId(),
		req.GetSpaceReadDecisionRef(), req.GetSpaceReadDecisionToken())
	if err != nil {
		return nil, err
	}
	if principal.PrincipalType == "user" {
		resolved, getErr := store.Get(ctx, principal.OrganizationID, req.GetProcessId())
		if getErr != nil {
			return nil, mapErr(getErr)
		}
		if resolved.SpaceID != req.GetSpaceId() {
			// Not found, not forbidden: telling a caller that the id exists in
			// a different Space is the disclosure this check exists to stop.
			return nil, mapErr(process.ErrProcessNotFound)
		}
	}
	page, err := store.ReadOutput(ctx, principal.OrganizationID, req.GetProcessId(), req.GetAfterSeq(), req.GetMaxBytes(), ceiling)
	if err != nil {
		return nil, mapErr(err)
	}
	chunks := make([]*ProcessOutputChunk, 0, len(page.Chunks))
	for _, c := range page.Chunks {
		chunks = append(chunks, &ProcessOutputChunk{
			Seq:             c.Seq,
			Stream:          mpv1.ProcessStream(c.Stream),
			Content:         c.Content,
			EndsWithNewline: c.EndsWithNewline,
			CapturedAt:      timestamppb.New(c.CapturedAt),
		})
	}
	return &ReadProcessOutputResponse{
		Chunks:          chunks,
		NextCursor:      page.NextCursor,
		GapBefore:       page.GapBefore,
		RetainedFromSeq: page.RetainedFromSeq,
		State:           mpv1.ProcessState(page.State),
		ExitCode:        page.ExitCode,
		EndReason:       page.EndReason,
		HasMore:         page.HasMore,
	}, nil
}

// processFence builds the write fence from the verified principal plus the
// request's host identity. The organization is never taken from the request.
func processFence(principal authctx.Principal, processID, backendID, hostEpoch string) (process.Fence, error) {
	if processID == "" {
		return process.Fence{}, status.Error(codes.InvalidArgument, "process_id is required")
	}
	if backendID == "" || hostEpoch == "" {
		return process.Fence{}, status.Error(codes.InvalidArgument, "backend_id and host_epoch are required")
	}
	return process.Fence{
		ProcessID: processID,
		OrgID:     principal.OrganizationID,
		BackendID: backendID,
		HostEpoch: hostEpoch,
	}, nil
}

// The three conversions below are direct casts because the proto enums were
// defined with the same numeric values the store uses, which are in turn the
// values migration 0003 stores. process_contract_test.go asserts that pairing
// explicitly rather than trusting it: the two definitions live in different
// files and nothing but that test would notice them drifting apart.

func processStateFromProto(state mpv1.ProcessState) (process.State, error) {
	converted := process.State(state)
	if !converted.IsLive() && !converted.IsTerminal() {
		return process.StateUnspecified, status.Errorf(codes.InvalidArgument, "unknown process state %v", state)
	}
	return converted, nil
}

func processSignalFromProto(signal mpv1.ProcessSignal) (process.Signal, error) {
	switch signal {
	case mpv1.ProcessSignal_PROCESS_SIGNAL_TERM:
		return process.SignalTerm, nil
	case mpv1.ProcessSignal_PROCESS_SIGNAL_KILL:
		return process.SignalKill, nil
	default:
		return process.SignalNone, status.Error(codes.InvalidArgument, "signal must be term or kill")
	}
}

func processStreamFromProto(stream mpv1.ProcessStream) (process.Stream, error) {
	switch stream {
	case mpv1.ProcessStream_PROCESS_STREAM_STDOUT:
		return process.StreamStdout, nil
	case mpv1.ProcessStream_PROCESS_STREAM_STDERR:
		return process.StreamStderr, nil
	case mpv1.ProcessStream_PROCESS_STREAM_SYSTEM:
		return process.StreamSystem, nil
	default:
		return 0, status.Error(codes.InvalidArgument, "chunk stream is not a known stream")
	}
}

func processToProto(p *process.Process) *Process {
	if p == nil {
		return nil
	}
	return &Process{
		ProcessId:       p.ID,
		OrgId:           p.OrgID,
		SpaceId:         p.SpaceID,
		LeaseId:         p.LeaseID,
		BackendId:       p.BackendID,
		HostEpoch:       p.HostEpoch,
		RunId:           p.RunID,
		StepId:          p.StepID,
		SubjectId:       p.SubjectID,
		Command:         &RedactedCommand{Program: p.Command.Program, Args: p.Command.Args},
		CommandDigest:   p.CommandDigest,
		State:           mpv1.ProcessState(p.State),
		ExitCode:        p.ExitCode,
		EndReason:       p.EndReason,
		SignalRequested: mpv1.ProcessSignal(p.SignalRequested),
		TermRequestedAt: optionalTimestamp(p.TermRequestedAt),
		CleanupDone:     p.CleanupState == process.CleanupDone,
		TtlSeconds:      p.TTLSeconds,
		ExpiresAt:       timestamppb.New(p.ExpiresAt),
		StartedAt:       optionalTimestamp(p.StartedAt),
		EndedAt:         optionalTimestamp(p.EndedAt),
		LastHeartbeatAt: timestamppb.New(p.LastHeartbeatAt),
		NextSeq:         p.NextSeq,
		RetainedFromSeq: p.RetainedFromSeq,
		RetainedBytes:   p.RetainedBytes,
		DroppedBytes:    p.DroppedBytes,
		StdinBytes:      p.StdinBytes,
		CreatedAt:       timestamppb.New(p.CreatedAt),
		UpdatedAt:       timestamppb.New(p.UpdatedAt),
		// Exposed so a reader other than this service can apply the ceiling —
		// S4.3's watch sweeper resolves a process on a service credential and
		// has to refuse content recorded under an audience its own decision
		// predates. A ceiling only one reader can see is a ceiling the next
		// reader silently does not have.
		RecipientAudienceRevision: p.AudienceRevision,
	}
}

func optionalTimestamp(value *time.Time) *timestamppb.Timestamp {
	if value == nil {
		return nil
	}
	return timestamppb.New(*value)
}

func timestampOrNow(value *timestamppb.Timestamp) time.Time {
	if value == nil {
		return time.Now().UTC()
	}
	return value.AsTime()
}
