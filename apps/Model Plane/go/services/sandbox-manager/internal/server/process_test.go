package server

import (
	"context"
	"errors"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/process"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/durationpb"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// stubProcessStore records what the handlers ask of the registry and returns
// scripted results.
//
// Deliberately a stub rather than an in-memory mirror of process.Store: the
// registry's own semantics — the write fence, ON CONFLICT dedup, retention,
// the lease eligibility gate — are already proven against real Postgres in
// internal/process's integration tests, and a second hand-written copy of
// them here would be a thing that can disagree with the real one rather than
// extra assurance. What these tests own is the handler layer: identity,
// validation, conversion, and error mapping.
type stubProcessStore struct {
	registerReq  process.RegisterRequest
	registerResp *process.Process
	registerErr  error

	startedFence process.Fence
	startedErr   error

	signalFence process.Fence
	signal      process.Signal
	signalErr   error

	endedFence       process.Fence
	endedState       process.State
	endedExitCode    *int32
	endedReason      string
	endedCleanupDone bool
	endedErr         error

	appendFence  process.Fence
	appendChunks []process.Chunk
	appendStdin  int64
	appendResp   *process.AppendResult
	appendErr    error

	reconcileBackend string
	reconcileEpoch   string
	reconcileResp    int64

	getOrg  string
	getID   string
	getResp *process.Process
	getErr  error

	listOrg      string
	listSpace    string
	listTerminal bool
	listLimit    int32
	listAfter    string
	listCeiling  *process.AudienceCeiling
	readCeiling  *process.AudienceCeiling
	listResp     []process.Process
	listHasMore  bool

	readOrg  string
	readID   string
	readSeq  int64
	readMax  int64
	readResp *process.OutputPage
	readErr  error

	killOrg     string
	killLease   string
	killCount   int64
	killErr     error
	killCalls   int
	registerHit int
}

func (s *stubProcessStore) Register(_ context.Context, req process.RegisterRequest) (*process.Process, error) {
	s.registerHit++
	s.registerReq = req
	if s.registerErr != nil {
		return nil, s.registerErr
	}
	if s.registerResp != nil {
		return s.registerResp, nil
	}
	return &process.Process{ID: req.ID, OrgID: req.OrgID, LeaseID: req.LeaseID, State: process.StateStarting}, nil
}

func (s *stubProcessStore) MarkStarted(_ context.Context, f process.Fence) error {
	s.startedFence = f
	return s.startedErr
}

func (s *stubProcessStore) RequestSignal(_ context.Context, f process.Fence, signal process.Signal) error {
	s.signalFence, s.signal = f, signal
	return s.signalErr
}

func (s *stubProcessStore) MarkEnded(_ context.Context, f process.Fence, state process.State, exitCode *int32, endReason string, cleanupDone bool) error {
	s.endedFence, s.endedState, s.endedExitCode, s.endedReason, s.endedCleanupDone = f, state, exitCode, endReason, cleanupDone
	return s.endedErr
}

func (s *stubProcessStore) AppendOutput(_ context.Context, f process.Fence, chunks []process.Chunk, stdinBytesDelta int64) (*process.AppendResult, error) {
	s.appendFence, s.appendChunks, s.appendStdin = f, chunks, stdinBytesDelta
	if s.appendErr != nil {
		return nil, s.appendErr
	}
	if s.appendResp != nil {
		return s.appendResp, nil
	}
	return &process.AppendResult{NextSeq: 1, RetainedFromSeq: 1}, nil
}

func (s *stubProcessStore) Reconcile(_ context.Context, backendID, hostEpoch string) (int64, error) {
	s.reconcileBackend, s.reconcileEpoch = backendID, hostEpoch
	return s.reconcileResp, nil
}

func (s *stubProcessStore) Get(_ context.Context, orgID, processID string) (*process.Process, error) {
	s.getOrg, s.getID = orgID, processID
	if s.getErr != nil {
		return nil, s.getErr
	}
	if s.getResp != nil {
		return s.getResp, nil
	}
	return &process.Process{ID: processID, OrgID: orgID, State: process.StateRunning}, nil
}

func (s *stubProcessStore) List(_ context.Context, orgID, spaceID string, includeTerminal bool, limit int32, afterID string, ceiling *process.AudienceCeiling) ([]process.Process, bool, error) {
	s.listOrg, s.listSpace, s.listTerminal, s.listLimit, s.listAfter = orgID, spaceID, includeTerminal, limit, afterID
	s.listCeiling = ceiling
	return s.listResp, s.listHasMore, nil
}

func (s *stubProcessStore) ReadOutput(_ context.Context, orgID, processID string, afterSeq, maxBytes int64, ceiling *process.AudienceCeiling) (*process.OutputPage, error) {
	s.readOrg, s.readID, s.readSeq, s.readMax = orgID, processID, afterSeq, maxBytes
	s.readCeiling = ceiling
	if s.readErr != nil {
		return nil, s.readErr
	}
	if s.readResp != nil {
		return s.readResp, nil
	}
	return &process.OutputPage{State: process.StateRunning}, nil
}

func (s *stubProcessStore) KillForLease(_ context.Context, orgID, leaseID string) (int64, error) {
	s.killCalls++
	s.killOrg, s.killLease = orgID, leaseID
	return s.killCount, s.killErr
}

func hostServer(store ProcessStore) *Server {
	srv := testServerFor(NewMemoryLeaseStore(), NewMemorySnapshotStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "service:execution-core", PrincipalType: "service",
	})
	return srv.WithProcessStore(store)
}

func userServer(store ProcessStore) *Server {
	srv := testServerFor(NewMemoryLeaseStore(), NewMemorySnapshotStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	})
	return srv.WithProcessStore(store)
}

func validRegisterRequest() *RegisterProcessRequest {
	return &RegisterProcessRequest{
		ProcessId:     "proc-1",
		LeaseId:       "lease-1",
		BackendId:     "backend-1",
		HostEpoch:     "epoch-1",
		RunId:         "run-1",
		StepId:        "step-1",
		SubjectId:     "user-1",
		Command:       &RedactedCommand{Program: "python3", Args: []string{"worker.py"}},
		CommandDigest: "sha256:abc",
		TtlSeconds:    900,
	}
}

// TestProcessEnumsMatchTheStoreNumbering is the cross-file contract this
// package's direct enum casts rely on. The proto values and the store's own
// constants are declared in different files and are both stored as raw
// numbers by migration 0003; nothing but this test would notice them
// drifting apart, and the symptom of drift would be a process silently
// reported in the wrong state.
func TestProcessEnumsMatchTheStoreNumbering(t *testing.T) {
	t.Parallel()
	states := map[mpv1.ProcessState]process.State{
		mpv1.ProcessState_PROCESS_STATE_UNSPECIFIED: process.StateUnspecified,
		mpv1.ProcessState_PROCESS_STATE_STARTING:    process.StateStarting,
		mpv1.ProcessState_PROCESS_STATE_RUNNING:     process.StateRunning,
		mpv1.ProcessState_PROCESS_STATE_EXITED:      process.StateExited,
		mpv1.ProcessState_PROCESS_STATE_KILLED:      process.StateKilled,
		mpv1.ProcessState_PROCESS_STATE_LOST:        process.StateLost,
		mpv1.ProcessState_PROCESS_STATE_EXPIRED:     process.StateExpired,
	}
	for wire, internal := range states {
		if int32(wire) != int32(internal) {
			t.Fatalf("proto %v = %d but store constant = %d", wire, int32(wire), int32(internal))
		}
	}
	signals := map[mpv1.ProcessSignal]process.Signal{
		mpv1.ProcessSignal_PROCESS_SIGNAL_UNSPECIFIED: process.SignalNone,
		mpv1.ProcessSignal_PROCESS_SIGNAL_TERM:        process.SignalTerm,
		mpv1.ProcessSignal_PROCESS_SIGNAL_KILL:        process.SignalKill,
	}
	for wire, internal := range signals {
		if int32(wire) != int32(internal) {
			t.Fatalf("proto %v = %d but store constant = %d", wire, int32(wire), int32(internal))
		}
	}
	streams := map[mpv1.ProcessStream]process.Stream{
		mpv1.ProcessStream_PROCESS_STREAM_STDOUT: process.StreamStdout,
		mpv1.ProcessStream_PROCESS_STREAM_STDERR: process.StreamStderr,
		mpv1.ProcessStream_PROCESS_STREAM_SYSTEM: process.StreamSystem,
	}
	for wire, internal := range streams {
		if int32(wire) != int32(internal) {
			t.Fatalf("proto %v = %d but store constant = %d", wire, int32(wire), int32(internal))
		}
	}
}

// TestProcessWritesRefuseAUserIdentity: the caller reporting what an OS
// process did is always an execution host's service identity. A user bearer
// asserting any of it would be asserting something it cannot observe.
func TestProcessWritesRefuseAUserIdentity(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := userServer(store)
	ctx := context.Background()

	calls := map[string]func() error{
		"RegisterProcess": func() error {
			_, err := s.RegisterProcess(ctx, validRegisterRequest())
			return err
		},
		"UpdateProcessState": func() error {
			_, err := s.UpdateProcessState(ctx, &UpdateProcessStateRequest{
				ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
				Transition: &mpv1.UpdateProcessStateRequest_Started{Started: &mpv1.ProcessStarted{}},
			})
			return err
		},
		"AppendProcessOutput": func() error {
			_, err := s.AppendProcessOutput(ctx, &AppendProcessOutputRequest{
				ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
			})
			return err
		},
		"ReconcileProcesses": func() error {
			_, err := s.ReconcileProcesses(ctx, &ReconcileProcessesRequest{BackendId: "backend-1", HostEpoch: "epoch-1"})
			return err
		},
	}
	for name, call := range calls {
		if got := status.Code(call()); got != codes.PermissionDenied {
			t.Fatalf("%s code = %v, want PermissionDenied", name, got)
		}
	}
	if store.registerHit != 0 {
		t.Fatal("a refused call still reached the registry")
	}
}

// admittedReadServer is a user-principal server whose Space read verifier
// always succeeds with the given ceiling — standing in for a Control decision
// the authz package already tests for real.
func admittedReadServer(store ProcessStore, revision int64) *Server {
	return userServer(store).WithSpaceReadVerifier(
		func(string, authz.SpaceReadExpectation) (authz.VerifiedSpaceRead, error) {
			return authz.VerifiedSpaceRead{RecipientAudienceRevision: revision}, nil
		},
	)
}

// refusedReadServer is a user-principal server whose verifier always refuses,
// standing in for an expired, mismatched or wrong-audience decision.
func refusedReadServer(store ProcessStore) *Server {
	return userServer(store).WithSpaceReadVerifier(
		func(string, authz.SpaceReadExpectation) (authz.VerifiedSpaceRead, error) {
			return authz.VerifiedSpaceRead{}, errors.New("decision does not authorize this read")
		},
	)
}

// TestProcessReadsRefuseAUserIdentityWithoutAConfiguredVerifier: an instance
// with no Control key cannot tell an authorized reader from an unauthorized
// one, so it serves neither. FailedPrecondition rather than PermissionDenied
// because the missing piece is deployment wiring, not the caller's authority —
// and the distinction matters to whoever has to debug it.
func TestProcessReadsRefuseAUserIdentityWithoutAConfiguredVerifier(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := userServer(store)
	ctx := context.Background()

	if _, err := s.ListProcesses(ctx, &ListProcessesRequest{SpaceId: "space-1"}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("ListProcesses code = %v, want FailedPrecondition", status.Code(err))
	}
	if _, err := s.ReadProcessOutput(ctx, &ReadProcessOutputRequest{ProcessId: "proc-1", SpaceId: "space-1"}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("ReadProcessOutput code = %v, want FailedPrecondition", status.Code(err))
	}
	if store.listOrg != "" || store.readOrg != "" {
		t.Fatal("a refused read still reached the registry")
	}
}

// TestGetProcessStaysServiceOnly: resolving one process by bare id is an
// execution host's operation — it is how execution-core checks a
// model-supplied id against its own Space before reading it. The human path
// pages a Space's list, so admitting a user here would add an authority path
// with no caller and the shape most useful for probing which ids exist.
func TestGetProcessStaysServiceOnly(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	// Even fully configured for human reads, GetProcess refuses.
	s := admittedReadServer(store, 4)

	if _, err := s.GetProcess(context.Background(), &GetProcessRequest{ProcessId: "proc-1"}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("GetProcess code = %v, want FailedPrecondition", status.Code(err))
	}
	if store.getOrg != "" {
		t.Fatal("a refused read still reached the registry")
	}
}

// TestProcessRPCsFailClosedWithoutARegistry covers ephemeral-development
// mode, where there is no durable registry and therefore no honest answer to
// give.
func TestProcessRPCsFailClosedWithoutARegistry(t *testing.T) {
	t.Parallel()
	s := hostServer(nil)
	ctx := context.Background()

	if _, err := s.RegisterProcess(ctx, validRegisterRequest()); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("RegisterProcess code = %v, want FailedPrecondition", status.Code(err))
	}
	if _, err := s.GetProcess(ctx, &GetProcessRequest{ProcessId: "proc-1"}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("GetProcess code = %v, want FailedPrecondition", status.Code(err))
	}
}

// TestRegisterProcessTakesTheTenantFromTheVerifiedIdentity: the request has
// no org field, and must not gain one.
func TestRegisterProcessTakesTheTenantFromTheVerifiedIdentity(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := hostServer(store)

	resp, err := s.RegisterProcess(context.Background(), validRegisterRequest())
	if err != nil {
		t.Fatalf("RegisterProcess: %v", err)
	}
	if store.registerReq.OrgID != "org-1" {
		t.Fatalf("OrgID = %q, want the principal's own", store.registerReq.OrgID)
	}
	if store.registerReq.Command.Program != "python3" || len(store.registerReq.Command.Args) != 1 {
		t.Fatalf("command not forwarded: %+v", store.registerReq.Command)
	}
	if resp.GetProcess().GetProcessId() != "proc-1" {
		t.Fatalf("response process = %+v", resp.GetProcess())
	}
	if resp.GetProcess().GetState() != mpv1.ProcessState_PROCESS_STATE_STARTING {
		t.Fatalf("state = %v, want STARTING", resp.GetProcess().GetState())
	}
}

func TestRegisterProcessValidatesItsRequest(t *testing.T) {
	t.Parallel()
	cases := map[string]func(*RegisterProcessRequest){
		"missing process_id": func(r *RegisterProcessRequest) { r.ProcessId = "" },
		"missing lease_id":   func(r *RegisterProcessRequest) { r.LeaseId = "" },
		"missing command":    func(r *RegisterProcessRequest) { r.Command = nil },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			store := &stubProcessStore{}
			s := hostServer(store)
			req := validRegisterRequest()
			mutate(req)
			if _, err := s.RegisterProcess(context.Background(), req); status.Code(err) != codes.InvalidArgument {
				t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
			}
			if store.registerHit != 0 {
				t.Fatal("an invalid request still reached the registry")
			}
		})
	}
}

// TestRegisterProcessMapsEveryRefusalToItsOwnCode: the three refusals mean
// materially different things to a caller — retry never, retry later, or
// retry when a slot frees — so they must not collapse into one code.
func TestRegisterProcessMapsEveryRefusalToItsOwnCode(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name string
		err  error
		want codes.Code
	}{
		{"not permitted", process.ErrProcessesNotPermitted, codes.PermissionDenied},
		{"lease not eligible", process.ErrLeaseNotEligible, codes.FailedPrecondition},
		{"limit reached", process.ErrProcessLimit, codes.ResourceExhausted},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			s := hostServer(&stubProcessStore{registerErr: tc.err})
			_, err := s.RegisterProcess(context.Background(), validRegisterRequest())
			if got := status.Code(err); got != tc.want {
				t.Fatalf("code = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestUpdateProcessStateDispatchesEachTransition(t *testing.T) {
	t.Parallel()
	exitCode := int32(3)

	t.Run("started", func(t *testing.T) {
		t.Parallel()
		store := &stubProcessStore{}
		s := hostServer(store)
		if _, err := s.UpdateProcessState(context.Background(), &UpdateProcessStateRequest{
			ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
			Transition: &mpv1.UpdateProcessStateRequest_Started{Started: &mpv1.ProcessStarted{}},
		}); err != nil {
			t.Fatalf("UpdateProcessState: %v", err)
		}
		want := process.Fence{ProcessID: "proc-1", OrgID: "org-1", BackendID: "backend-1", HostEpoch: "epoch-1"}
		if store.startedFence != want {
			t.Fatalf("fence = %+v, want %+v", store.startedFence, want)
		}
	})

	t.Run("signal", func(t *testing.T) {
		t.Parallel()
		store := &stubProcessStore{}
		s := hostServer(store)
		if _, err := s.UpdateProcessState(context.Background(), &UpdateProcessStateRequest{
			ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
			Transition: &mpv1.UpdateProcessStateRequest_Signal{
				Signal: &mpv1.ProcessSignalRequested{Signal: mpv1.ProcessSignal_PROCESS_SIGNAL_KILL},
			},
		}); err != nil {
			t.Fatalf("UpdateProcessState: %v", err)
		}
		if store.signal != process.SignalKill {
			t.Fatalf("signal = %v, want kill", store.signal)
		}
	})

	t.Run("exited", func(t *testing.T) {
		t.Parallel()
		store := &stubProcessStore{}
		s := hostServer(store)
		if _, err := s.UpdateProcessState(context.Background(), &UpdateProcessStateRequest{
			ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
			Transition: &mpv1.UpdateProcessStateRequest_Exited{Exited: &mpv1.ProcessExited{
				State:       mpv1.ProcessState_PROCESS_STATE_EXITED,
				ExitCode:    &exitCode,
				EndReason:   process.EndExited,
				CleanupDone: true,
			}},
		}); err != nil {
			t.Fatalf("UpdateProcessState: %v", err)
		}
		if store.endedState != process.StateExited || store.endedReason != process.EndExited {
			t.Fatalf("ended = %v/%q", store.endedState, store.endedReason)
		}
		if store.endedExitCode == nil || *store.endedExitCode != 3 {
			t.Fatalf("exit code = %v, want 3", store.endedExitCode)
		}
		if !store.endedCleanupDone {
			t.Fatal("cleanup_done not forwarded")
		}
	})
}

func TestUpdateProcessStateRejectsBadInput(t *testing.T) {
	t.Parallel()
	cases := map[string]*UpdateProcessStateRequest{
		"no transition": {ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1"},
		"missing fence": {ProcessId: "proc-1", Transition: &mpv1.UpdateProcessStateRequest_Started{Started: &mpv1.ProcessStarted{}}},
		"unknown signal": {
			ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
			Transition: &mpv1.UpdateProcessStateRequest_Signal{Signal: &mpv1.ProcessSignalRequested{}},
		},
		"non-terminal exit": {
			ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
			Transition: &mpv1.UpdateProcessStateRequest_Exited{Exited: &mpv1.ProcessExited{
				State: mpv1.ProcessState_PROCESS_STATE_RUNNING, EndReason: process.EndExited,
			}},
		},
	}
	for name, req := range cases {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			s := hostServer(&stubProcessStore{})
			if _, err := s.UpdateProcessState(context.Background(), req); status.Code(err) != codes.InvalidArgument {
				t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
			}
		})
	}
}

// TestUpdateProcessStateSurfacesAFencedWrite: a superseded host must be told
// to stop, not handed a success it can keep building on.
func TestUpdateProcessStateSurfacesAFencedWrite(t *testing.T) {
	t.Parallel()
	s := hostServer(&stubProcessStore{startedErr: process.ErrProcessFenced})
	_, err := s.UpdateProcessState(context.Background(), &UpdateProcessStateRequest{
		ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
		Transition: &mpv1.UpdateProcessStateRequest_Started{Started: &mpv1.ProcessStarted{}},
	})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestAppendProcessOutputConvertsChunksAndFence(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{appendResp: &process.AppendResult{
		NextSeq: 3, RetainedFromSeq: 1, RetainedBytes: 11, DroppedBytes: 0,
	}}
	s := hostServer(store)
	captured := time.Unix(1700000000, 0).UTC()

	resp, err := s.AppendProcessOutput(context.Background(), &AppendProcessOutputRequest{
		ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
		Chunks: []*ProcessOutputChunk{
			{Seq: 1, Stream: mpv1.ProcessStream_PROCESS_STREAM_STDOUT, Content: []byte("hello\n"), EndsWithNewline: true, CapturedAt: timestamppb.New(captured)},
			{Seq: 2, Stream: mpv1.ProcessStream_PROCESS_STREAM_STDERR, Content: []byte("warn\n"), EndsWithNewline: true},
		},
		StdinBytesDelta: 12,
	})
	if err != nil {
		t.Fatalf("AppendProcessOutput: %v", err)
	}
	if resp.GetNextSeq() != 3 || resp.GetRetainedBytes() != 11 {
		t.Fatalf("response = %+v", resp)
	}
	if len(store.appendChunks) != 2 {
		t.Fatalf("chunks = %d, want 2", len(store.appendChunks))
	}
	if store.appendChunks[0].Stream != process.StreamStdout || store.appendChunks[1].Stream != process.StreamStderr {
		t.Fatalf("streams not converted: %+v", store.appendChunks)
	}
	if !store.appendChunks[0].CapturedAt.Equal(captured) {
		t.Fatalf("captured_at = %v, want %v", store.appendChunks[0].CapturedAt, captured)
	}
	// A chunk with no timestamp still gets one rather than a zero time.
	if store.appendChunks[1].CapturedAt.IsZero() {
		t.Fatal("missing captured_at was not defaulted")
	}
	if store.appendStdin != 12 {
		t.Fatalf("stdin delta = %d, want 12", store.appendStdin)
	}
}

// TestAppendProcessOutputWithNoChunksIsAllowed: an empty batch is the
// heartbeat, so the handler must not reject it as empty input.
func TestAppendProcessOutputWithNoChunksIsAllowed(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := hostServer(store)
	if _, err := s.AppendProcessOutput(context.Background(), &AppendProcessOutputRequest{
		ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
	}); err != nil {
		t.Fatalf("a heartbeat was rejected: %v", err)
	}
	if len(store.appendChunks) != 0 {
		t.Fatalf("chunks = %d, want none", len(store.appendChunks))
	}
}

func TestAppendProcessOutputRejectsAnUnknownStream(t *testing.T) {
	t.Parallel()
	s := hostServer(&stubProcessStore{})
	_, err := s.AppendProcessOutput(context.Background(), &AppendProcessOutputRequest{
		ProcessId: "proc-1", BackendId: "backend-1", HostEpoch: "epoch-1",
		Chunks: []*ProcessOutputChunk{{Seq: 1, Stream: mpv1.ProcessStream_PROCESS_STREAM_UNSPECIFIED}},
	})
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
}

func TestReconcileProcessesRequiresItsHostIdentity(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{reconcileResp: 2}
	s := hostServer(store)

	if _, err := s.ReconcileProcesses(context.Background(), &ReconcileProcessesRequest{BackendId: "backend-1"}); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
	resp, err := s.ReconcileProcesses(context.Background(), &ReconcileProcessesRequest{BackendId: "backend-1", HostEpoch: "epoch-2"})
	if err != nil {
		t.Fatalf("ReconcileProcesses: %v", err)
	}
	if resp.GetLostCount() != 2 {
		t.Fatalf("lost = %d, want 2", resp.GetLostCount())
	}
	if store.reconcileEpoch != "epoch-2" {
		t.Fatalf("epoch = %q", store.reconcileEpoch)
	}
}

// TestProcessReadsAreScopedToTheVerifiedOrganization: no read takes a tenant
// from the request.
func TestProcessReadsAreScopedToTheVerifiedOrganization(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{
		listResp:    []process.Process{{ID: "proc-2", State: process.StateRunning}},
		listHasMore: true,
		readResp: &process.OutputPage{
			Chunks:     []process.Chunk{{Seq: 1, Stream: process.StreamStdout, Content: []byte("x"), CapturedAt: time.Unix(1, 0).UTC()}},
			NextCursor: 1,
			GapBefore:  true,
			State:      process.StateExited,
			EndReason:  process.EndExited,
		},
	}
	s := hostServer(store)
	ctx := context.Background()

	if _, err := s.GetProcess(ctx, &GetProcessRequest{ProcessId: "proc-1"}); err != nil {
		t.Fatalf("GetProcess: %v", err)
	}
	if store.getOrg != "org-1" {
		t.Fatalf("Get org = %q", store.getOrg)
	}

	listed, err := s.ListProcesses(ctx, &ListProcessesRequest{SpaceId: "space-1", IncludeTerminal: true, Limit: 5, AfterProcessId: "proc-9"})
	if err != nil {
		t.Fatalf("ListProcesses: %v", err)
	}
	if store.listOrg != "org-1" || store.listSpace != "space-1" || !store.listTerminal || store.listLimit != 5 || store.listAfter != "proc-9" {
		t.Fatalf("list args = %q/%q/%v/%d/%q", store.listOrg, store.listSpace, store.listTerminal, store.listLimit, store.listAfter)
	}
	if len(listed.GetProcesses()) != 1 || !listed.GetHasMore() {
		t.Fatalf("list response = %+v", listed)
	}

	page, err := s.ReadProcessOutput(ctx, &ReadProcessOutputRequest{ProcessId: "proc-1", AfterSeq: 4, MaxBytes: 16})
	if err != nil {
		t.Fatalf("ReadProcessOutput: %v", err)
	}
	if store.readOrg != "org-1" || store.readSeq != 4 || store.readMax != 16 {
		t.Fatalf("read args = %q/%d/%d", store.readOrg, store.readSeq, store.readMax)
	}
	if !page.GetGapBefore() {
		t.Fatal("gap_before was not carried to the wire")
	}
	if page.GetState() != mpv1.ProcessState_PROCESS_STATE_EXITED || page.GetEndReason() != process.EndExited {
		t.Fatalf("terminal outcome lost: %v/%q", page.GetState(), page.GetEndReason())
	}
	if len(page.GetChunks()) != 1 || page.GetChunks()[0].GetStream() != mpv1.ProcessStream_PROCESS_STREAM_STDOUT {
		t.Fatalf("chunks = %+v", page.GetChunks())
	}
}

func TestGetProcessMapsNotFound(t *testing.T) {
	t.Parallel()
	s := hostServer(&stubProcessStore{getErr: process.ErrProcessNotFound})
	if _, err := s.GetProcess(context.Background(), &GetProcessRequest{ProcessId: "proc-1"}); status.Code(err) != codes.NotFound {
		t.Fatalf("code = %v, want NotFound", status.Code(err))
	}
}

// TestReleaseLeaseTerminatesItsProcesses: a released lease must leave no row
// claiming to still be running.
func TestReleaseLeaseTerminatesItsProcesses(t *testing.T) {
	t.Parallel()
	leases := NewMemoryLeaseStore()
	store := &stubProcessStore{killCount: 1}
	srv := testServerFor(leases, NewMemorySnapshotStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	}).WithProcessStore(store)

	acquired, err := srv.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	if _, err := srv.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acquired.GetLeaseId()}); err != nil {
		t.Fatalf("ReleaseLease: %v", err)
	}
	if store.killCalls != 1 {
		t.Fatalf("KillForLease calls = %d, want 1", store.killCalls)
	}
	if store.killOrg != "org-1" || store.killLease != acquired.GetLeaseId() {
		t.Fatalf("kill args = %q/%q", store.killOrg, store.killLease)
	}
}

// TestReleaseLeaseSucceedsEvenIfTerminatingItsProcessesFails: the lease IS
// gone, and the staleness sweeper catches whatever this missed. Failing the
// release would leave the caller believing it still holds one.
func TestReleaseLeaseSucceedsEvenIfTerminatingItsProcessesFails(t *testing.T) {
	t.Parallel()
	leases := NewMemoryLeaseStore()
	store := &stubProcessStore{killErr: errors.New("connection reset")}
	srv := testServerFor(leases, NewMemorySnapshotStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	}).WithProcessStore(store)

	acquired, err := srv.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	released, err := srv.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acquired.GetLeaseId()})
	if err != nil {
		t.Fatalf("ReleaseLease should not fail on a registry error: %v", err)
	}
	if !released.GetReleased() {
		t.Fatal("lease was not reported released")
	}
}

// TestListProcessesAppliesTheReadersAudienceCeiling: the decision does not
// merely admit the caller, it bounds what they see. A member holding a valid
// decision must not read work recorded under an audience their decision
// predates, which is the same rule Session Core applies to runs.
func TestListProcessesAppliesTheReadersAudienceCeiling(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := admittedReadServer(store, 4)

	if _, err := s.ListProcesses(context.Background(), &ListProcessesRequest{
		SpaceId: "space-1", SpaceReadDecisionRef: "read-1", SpaceReadDecisionToken: "token",
	}); err != nil {
		t.Fatalf("ListProcesses: %v", err)
	}
	if store.listCeiling == nil {
		t.Fatal("a human read reached the registry with no ceiling at all; every process in the Space would be returned")
	}
	if store.listCeiling.RecipientAudienceRevision != 4 {
		t.Fatalf("ceiling = %d, want the decision's own revision 4", store.listCeiling.RecipientAudienceRevision)
	}
}

// TestServiceReadsStayUnbounded: execution-core is not a disclosure recipient
// — it is the thing that produced these records, and it applies its own Space
// check before showing anything to a model. Giving it a ceiling it has no
// decision to derive would break the model-facing tools for no gain.
func TestServiceReadsStayUnbounded(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := testServerFor(NewMemoryLeaseStore(), NewMemorySnapshotStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "execution-core", PrincipalType: "service",
	}).WithProcessStore(store)

	if _, err := s.ListProcesses(context.Background(), &ListProcessesRequest{SpaceId: "space-1"}); err != nil {
		t.Fatalf("ListProcesses: %v", err)
	}
	if store.listCeiling != nil {
		t.Fatalf("a service read was bounded at %d; it has no decision to derive one from", store.listCeiling.RecipientAudienceRevision)
	}
}

// TestProcessReadsRefuseARejectedDecision: PermissionDenied, and the registry
// is never touched. An expired, mismatched, or wrong-audience decision is a
// statement about the caller's authority, unlike the unconfigured case above.
func TestProcessReadsRefuseARejectedDecision(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{}
	s := refusedReadServer(store)
	ctx := context.Background()

	if _, err := s.ListProcesses(ctx, &ListProcessesRequest{
		SpaceId: "space-1", SpaceReadDecisionRef: "read-1", SpaceReadDecisionToken: "token",
	}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("ListProcesses code = %v, want PermissionDenied", status.Code(err))
	}
	if _, err := s.ReadProcessOutput(ctx, &ReadProcessOutputRequest{
		ProcessId: "proc-1", SpaceId: "space-1", SpaceReadDecisionRef: "read-1", SpaceReadDecisionToken: "token",
	}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("ReadProcessOutput code = %v, want PermissionDenied", status.Code(err))
	}
	if store.listOrg != "" || store.readOrg != "" {
		t.Fatal("a refused read still reached the registry")
	}
}

// TestReadProcessOutputRefusesAProcessOutsideTheDecisionsSpace: the decision
// authorizes ONE Space and a process id names none, so the Space comes from
// the request and the row is checked against it. Reading the Space off the row
// and verifying the decision against THAT would let a caller present a
// decision for a Space they belong to and have it checked against itself.
//
// NotFound rather than PermissionDenied on purpose: distinguishing "exists
// elsewhere" from "does not exist" is exactly the probe this prevents.
func TestReadProcessOutputRefusesAProcessOutsideTheDecisionsSpace(t *testing.T) {
	t.Parallel()
	store := &stubProcessStore{
		getResp: &process.Process{ID: "proc-1", OrgID: "org-1", SpaceID: "other-space"},
	}
	s := admittedReadServer(store, 4)

	_, err := s.ReadProcessOutput(context.Background(), &ReadProcessOutputRequest{
		ProcessId: "proc-1", SpaceId: "space-1",
		SpaceReadDecisionRef: "read-1", SpaceReadDecisionToken: "token",
	})
	if status.Code(err) != codes.NotFound {
		t.Fatalf("code = %v, want NotFound — a different code tells the caller the id exists somewhere", status.Code(err))
	}
	if store.readOrg != "" {
		t.Fatal("output was read for a process outside the decision's Space")
	}
}
