package activities

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/zdrrefusal"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.temporal.io/sdk/temporal"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// memoryGateOrchestratorWritesEnv names the switch that sends consolidated-memory
// writes through Session Core's content policy before letta-bridge indexes them.
const memoryGateOrchestratorWritesEnv = "MEMORY_GATE_ORCHESTRATOR_WRITES"

// MemoryGateOrchestratorWritesFromLookup reads MEMORY_GATE_ORCHESTRATOR_WRITES
// strictly: only the literal "true" turns the content gate on, so a typo or a
// stray "1" leaves WriteConsolidatedMemoryActivity exactly as it was.
func MemoryGateOrchestratorWritesFromLookup(lookup func(string) string) bool {
	return lookup(memoryGateOrchestratorWritesEnv) == "true"
}

const (
	// consolidatedMemoryScope is the scope a consolidated entry is checked as. The
	// entry is a thread's memory (letta-bridge keys it by thread), and Session
	// Core refuses every non-person origin that asks for "org", so asking for org
	// would refuse all consolidated writes as scope_not_permitted.
	consolidatedMemoryScope = "thread"

	// consolidatedMemoryOrigin is the origin Session Core's policy reads: an
	// orchestrator workflow's write. It matches the origin IndexMemory carries.
	consolidatedMemoryOrigin = "workflow"

	// memoryWriteCheckTimeout bounds one policy question so a stalled Session
	// Core turns into a failed activity attempt instead of a hung one.
	memoryWriteCheckTimeout = 3 * time.Second

	// memoryWriteRuleNone and memoryWriteRuleOther are the counter's rule labels
	// for an allowed entry and for a refusal token this build does not know.
	memoryWriteRuleNone  = "none"
	memoryWriteRuleOther = "other"

	// memoryContentRefusedType is the Temporal application-error type of a
	// consolidation whose every entry the content policy refused.
	memoryContentRefusedType = "MemoryContentRefused"

	// zdrRefusedType is the Temporal application-error type of a consolidation
	// refused for its retention posture, carrying the catalogue's refusal.
	zdrRefusedType = "ZDRRefused"
)

// memoryWriteChecker is the slice of Session Core's MemoryService this activity
// uses: the content policy question that stores nothing.
type memoryWriteChecker interface {
	CheckMemoryWrite(ctx context.Context, in *mpv1.CheckMemoryWriteRequest, opts ...grpc.CallOption) (*mpv1.CheckMemoryWriteResponse, error)
}

// SetMemoryContentGate turns the Session Core content gate on or off for
// WriteConsolidatedMemoryActivity. Off, the activity is unchanged.
func (a *Activities) SetMemoryContentGate(enabled bool) { a.gateConsolidatedWrites = enabled }

// contentPolicy is the client that answers the write question: the injected
// checker if one was set, otherwise Session Core's MemoryService. nil means
// Session Core cannot be asked at all.
func (a *Activities) contentPolicy() memoryWriteChecker {
	if a.memoryWriteCheck != nil {
		return a.memoryWriteCheck
	}
	if a.clients == nil || a.clients.SessionCore == nil {
		return nil
	}
	return mpv1.NewMemoryServiceClient(a.clients.SessionCore)
}

// knownMemoryWriteRule maps Session Core's refusal token onto a bounded counter
// label. An empty token is a refusal that named no rule.
func knownMemoryWriteRule(code string) string {
	switch code {
	case "injection_marker", "hidden_characters", "secret_like", "scope_not_permitted", "too_long":
		return code
	case "":
		return "refused"
	default:
		return memoryWriteRuleOther
	}
}

// gateConsolidatedEntries applies the Session Core content gate to a
// consolidation and returns the entries that may be indexed.
//
// It fails closed at every step, and nothing is written by this function:
//   - A consolidation that is not attested durable (zero data retention, waived,
//     or never told) is refused with the catalogue's durable-mutation refusal,
//     before Session Core is asked anything.
//   - If Session Core cannot be asked about any entry, the whole activity fails
//     (retryable) and the caller indexes nothing, because every entry is checked
//     before the first is written.
//   - A refused entry is skipped and counted; the activity fails (non-retryable)
//     only when every entry was refused.
//
// Entry content travels to the policy and is never logged: logs carry counts and
// the refusal rule only.
func (a *Activities) gateConsolidatedEntries(ctx context.Context, input WriteMemoryInput) ([]MemoryEntry, error) {
	if input.Retention != RetentionDurable {
		return nil, temporal.NewNonRetryableApplicationError(
			zdrrefusal.Message(zdrrefusal.DurableMutationForbidden, ""),
			zdrRefusedType,
			zdrrefusal.Status(zdrrefusal.DurableMutationForbidden, ""),
		)
	}
	if len(input.Entries) == 0 {
		return nil, nil
	}
	checker := a.contentPolicy()
	if checker == nil {
		return nil, status.Error(codes.Unavailable, "session-core unavailable")
	}

	allowed := make([]MemoryEntry, 0, len(input.Entries))
	refusedByRule := map[string]int{}
	for _, entry := range input.Entries {
		verdict, err := askMemoryWrite(ctx, checker, entry)
		if err != nil {
			return nil, fmt.Errorf("session-core memory content check: %w", err)
		}
		if verdict.GetAllowed() {
			telemetry.MemoryWriteGateEntriesTotal.Add(ctx, 1, metric.WithAttributes(
				attribute.String("outcome", "allowed"),
				attribute.String("rule", memoryWriteRuleNone),
			))
			allowed = append(allowed, entry)
			continue
		}
		rule := knownMemoryWriteRule(verdict.GetRefusalCode())
		refusedByRule[rule]++
		telemetry.MemoryWriteGateEntriesTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("outcome", "refused"),
			attribute.String("rule", rule),
		))
	}

	refused := len(input.Entries) - len(allowed)
	if refused > 0 {
		rules := make([]string, 0, len(refusedByRule))
		for rule := range refusedByRule {
			rules = append(rules, rule)
		}
		sort.Strings(rules)
		a.logger.Warn("consolidated memory entries refused by the content policy",
			"entries", len(input.Entries), "refused", refused, "rules", strings.Join(rules, ","))
	}
	if len(allowed) == 0 {
		return nil, temporal.NewNonRetryableApplicationError(
			"every consolidated memory entry was refused by the content policy",
			memoryContentRefusedType,
			nil,
		)
	}
	return allowed, nil
}

// askMemoryWrite asks Session Core's CheckMemoryWrite about one entry. The
// consolidation has no acting person, so the question is asked as the reserved
// system actor; Session Core's policy does not read the user.
func askMemoryWrite(ctx context.Context, checker memoryWriteChecker, entry MemoryEntry) (*mpv1.CheckMemoryWriteResponse, error) {
	callCtx, cancel := context.WithTimeout(ctx, memoryWriteCheckTimeout)
	defer cancel()
	return checker.CheckMemoryWrite(callCtx, &mpv1.CheckMemoryWriteRequest{
		OrgId:   entry.OrgID,
		UserId:  SystemActorID,
		Content: entry.Content,
		Scope:   consolidatedMemoryScope,
		Origin:  consolidatedMemoryOrigin,
	})
}
