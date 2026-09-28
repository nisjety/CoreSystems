package processwatch

import (
	"context"
	"fmt"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/watch"
)

// ErrSourceGone is the core's sentinel, re-exported so this package's callers
// and tests do not have to reach past it.
//
// Defined in the watch package rather than here because the sweeper is what
// acts on it, and the core cannot import an adapter. Returning it is how an
// adapter says "end this watch"; every other error means "try again later".
var ErrSourceGone = watch.ErrSourceGone

// TokenProvider mints capability-core's own `aud=sandbox-manager` credential.
type TokenProvider interface {
	Token(ctx context.Context, orgID string) (string, error)
}

// registryClient is the slice of the generated SandboxManager client this
// adapter uses, so the poll logic is testable against a fake.
//
// Declared with the generated signature (variadic grpc.CallOption) so
// mpv1.SandboxManagerClient satisfies it directly — no wrapper to keep in sync.
type registryClient interface {
	GetProcess(ctx context.Context, in *mpv1.GetProcessRequest, opts ...grpc.CallOption) (*mpv1.GetProcessResponse, error)
	ReadProcessOutput(ctx context.Context, in *mpv1.ReadProcessOutputRequest, opts ...grpc.CallOption) (*mpv1.ReadProcessOutputResponse, error)
}

// Adapter reads a background process's output for the watch sweeper.
type Adapter struct {
	client registryClient
	tokens TokenProvider
}

// NewAdapter constructs the process-output source.
func NewAdapter(client registryClient, tokens TokenProvider) (*Adapter, error) {
	if client == nil || tokens == nil {
		return nil, fmt.Errorf("the process-output adapter requires a registry client and a token provider")
	}
	return &Adapter{client: client, tokens: tokens}, nil
}

// Kind implements watch.SourceAdapter.
func (a *Adapter) Kind() string { return watch.SourceKindProcessOutput }

// Poll reads the process at the watch's cursor.
//
// # The Space check, which is the security property of this adapter
//
// capability-core's sandbox-manager credential is ORG-WIDE, exactly like
// execution-core's: sandbox-manager will resolve any process id in the
// organization for it, and correctly so — the caller is a service, not a
// disclosure recipient. `source_ref` is caller-supplied.
//
// So the only thing standing between a watch naming an arbitrary process id and
// another Space's output is the comparison below: resolve the row, refuse
// unless its `space_id` equals the watch's own. The watch's Space is
// trustworthy because Control signed it when the watch was created; the
// `source_ref` is not.
//
// # The audience ceiling
//
// A watch created under audience revision N must not surface content recorded
// under a later audience. The process row carries its own
// `recipient_audience_revision` (S4.2 migration 0004) and the watch carries the
// one its Control decision was signed under, so the comparison is available
// here and is made here.
func (a *Adapter) Poll(ctx context.Context, w watch.Watch) (watch.PollResult, error) {
	token, err := a.tokens.Token(ctx, w.OrgID)
	if err != nil {
		return watch.PollResult{}, fmt.Errorf("mint a sandbox-manager credential: %w", err)
	}
	authorized := metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+token)

	resolved, err := a.client.GetProcess(authorized, &mpv1.GetProcessRequest{ProcessId: w.SourceRef})
	if err != nil {
		if status.Code(err) == codes.NotFound {
			return watch.PollResult{}, ErrSourceGone
		}
		return watch.PollResult{}, fmt.Errorf("resolve the watched process: %w", err)
	}
	process := resolved.GetProcess()
	if process == nil || process.GetSpaceId() != w.SpaceRef {
		return watch.PollResult{}, ErrSourceGone
	}
	if ceiling := w.Authority.RecipientAudienceRevision; ceiling > 0 &&
		process.GetRecipientAudienceRevision() > ceiling {
		// Recorded under an audience this watch's decision predates. Reported as
		// gone rather than as a refusal, for the same no-oracle reason as above.
		return watch.PollResult{}, ErrSourceGone
	}

	terminal := processIsTerminal(process.GetState())
	page, err := a.client.ReadProcessOutput(authorized, &mpv1.ReadProcessOutputRequest{
		ProcessId: w.SourceRef,
		AfterSeq:  w.CursorValue,
		MaxBytes:  MaxReadBytes,
		// The Space the caller claims, which sandbox-manager checks against the
		// row. Harmless for a service principal, which reads unbounded, but
		// sent anyway so the request says what it means and a future tightening
		// of that RPC does not silently start refusing this caller.
		SpaceId: w.SpaceRef,
	})
	if err != nil {
		if status.Code(err) == codes.NotFound {
			return watch.PollResult{}, ErrSourceGone
		}
		return watch.PollResult{}, fmt.Errorf("read process output: %w", err)
	}

	// A terminal process is only DRAINED once a read comes back with nothing
	// left. Reporting terminal while output remains would end the watch before
	// its last lines — which is exactly where a failing build says why it
	// failed.
	drained := terminal && len(page.GetChunks()) == 0

	lines, err := assembleLines(page.GetChunks(), w.CursorValue, drained)
	if err != nil {
		return watch.PollResult{}, err
	}

	cursor := lines.cursor
	if drained {
		// Nothing was read, so the registry's own next cursor is authoritative.
		if page.GetNextCursor() > cursor {
			cursor = page.GetNextCursor()
		}
	}

	return watch.PollResult{
		Lines:     lines.lines,
		Cursor:    cursor,
		GapBefore: page.GetGapBefore(),
		// Only once drained. See `drained` above.
		SourceTerminal:  drained,
		TerminalSummary: terminalSummary(process),
	}, nil
}

func processIsTerminal(state mpv1.ProcessState) bool {
	switch state {
	case mpv1.ProcessState_PROCESS_STATE_EXITED,
		mpv1.ProcessState_PROCESS_STATE_KILLED,
		mpv1.ProcessState_PROCESS_STATE_EXPIRED,
		mpv1.ProcessState_PROCESS_STATE_LOST:
		return true
	default:
		return false
	}
}

// terminalSummary is the owning plane's own account of how the process ended.
//
// Assembled ONLY from registry fields — state, exit code, end reason — and
// never from output. It is emitted as `owner_metadata`, and the emission
// validator refuses that label on anything derived from watched content, so
// putting a line in here would be caught; stating the rule at the source is
// cheaper than relying on being caught.
func terminalSummary(process *mpv1.Process) string {
	if process == nil {
		return ""
	}
	switch process.GetState() {
	case mpv1.ProcessState_PROCESS_STATE_EXITED:
		if process.ExitCode != nil {
			if process.GetExitCode() == 0 {
				return "the process finished successfully"
			}
			return fmt.Sprintf("the process exited with code %d", process.GetExitCode())
		}
		return "the process exited"
	case mpv1.ProcessState_PROCESS_STATE_KILLED:
		return "the process was stopped"
	case mpv1.ProcessState_PROCESS_STATE_EXPIRED:
		return "the process reached its time limit and was stopped"
	case mpv1.ProcessState_PROCESS_STATE_LOST:
		// The honest one: nobody can say what happened. The room's grammar
		// ranks this above a plain failure for the same reason.
		return "the host running this process went away; its outcome is unknown"
	default:
		reason := strings.TrimSpace(process.GetEndReason())
		if reason == "" {
			return ""
		}
		return "the process ended: " + reason
	}
}
