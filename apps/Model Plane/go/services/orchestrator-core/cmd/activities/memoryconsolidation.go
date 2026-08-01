package activities

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// MemoryConsolidationModel is the model that consolidates a thread's memory.
//
// A CONCRETE model id, deliberately not a `velion-*` intent tier. Those tiers are
// re-routed by prompt size, and a fixed-token micro-call like this one lands on a
// reasoning model that answers HTTP 200 with an empty body — the failure looks
// like "the model had nothing to say" rather than a routing mistake. Overridable
// per deployment, but the default is pinned.
var MemoryConsolidationModel = envOrDefault(
	"MEMORY_CONSOLIDATION_MODEL", "gpt-4o-mini")

const (
	// consolidationMaxTokens bounds one thread's consolidated memory. Generous
	// enough that the model is not cut mid-sentence — the previous deterministic
	// path capped output at 512 BYTES, which is what made its summaries useless.
	consolidationMaxTokens = 512

	// consolidationTimeout bounds one thread's call. Consolidation is a
	// background sweep, so a slow provider must not hold the activity open long
	// enough to hit Temporal's start-to-close and retry the whole batch.
	consolidationTimeout = 20 * time.Second

	// maxConsolidationCalls bounds the model calls one activity will make.
	// Threads beyond it keep their deterministic summary rather than the sweep
	// silently costing one inference per thread on a large org.
	maxConsolidationCalls = 25

	// consolidationTemperature is 0: this is extraction, and a creative
	// consolidation would be inventing memories.
	consolidationTemperature = 0.0
)

func envOrDefault(key, fallback string) string {
	if v := strings.TrimSpace(os.Getenv(key)); v != "" {
		return v
	}
	return fallback
}

// consolidationPrompt asks for durable facts rather than prose.
//
// The instruction to drop everything not stated is the important half: a summary
// that smooths over gaps produces confident memories the conversation never
// supported, and those then ground future answers.
func consolidationPrompt(transcript string) string {
	return "Below are memory entries from one conversation thread.\n\n" +
		"Consolidate them into a compact list of DURABLE facts and preferences worth " +
		"remembering about this user or organisation. Rules:\n" +
		"- One fact per line, no numbering, no preamble.\n" +
		"- Keep names, numbers, dates and identifiers exactly as written.\n" +
		"- Merge duplicates and supersede stale statements with newer ones.\n" +
		"- Drop small talk, one-off questions, and anything transient.\n" +
		"- Write ONLY what the entries state. Do not infer, generalise or fill gaps.\n" +
		"- Answer in the language the entries are written in.\n" +
		"- If nothing is worth remembering, answer exactly: NOTHING\n\n" +
		"Entries:\n" + transcript
}

// consolidationNothing is the sentinel that means "no durable memory here".
//
// Needed because an empty completion is ambiguous — it is also what a failed or
// truncated call returns — and writing an empty memory row back would erase a
// thread's memory rather than leave it alone.
const consolidationNothing = "NOTHING"

// SummarizeMemoryActivity consolidates memory entries thread by thread.
//
// The deterministic grouping still runs first and is the fallback: it decides
// which threads exist and provides each one's summary if the model cannot be
// reached. Then each group is offered to inference-core, and a per-thread failure
// degrades only that thread. The activity therefore never fails the workflow over
// a provider problem — a consolidation sweep that returns the previous behaviour
// is strictly better than one that returns an error and retries the batch.
//
// Persistence is already authorized by the time this runs: MemoryConsolidation is
// registered `DeniesZDR: true`, so orchestrator-core's entry gate has refused any
// caller without an explicitly attested non-ZDR posture. That is what makes it
// safe to send this content to a model and write the result back.
func (a *Activities) SummarizeMemoryActivity(
	ctx context.Context,
	input ConsolidationInput,
) (ConsolidationOutput, error) {
	grouped := summarizeMemoryEntries(input.Entries)
	if len(grouped) == 0 {
		return ConsolidationOutput{Summary: "no entries to consolidate"}, nil
	}
	if a.clients == nil || a.clients.InferenceCore == nil {
		return ConsolidationOutput{
			ConsolidatedEntries: grouped,
			Summary: fmt.Sprintf(
				"consolidated %d entries into %d summaries (deterministic: inference-core unavailable)",
				len(input.Entries), len(grouped)),
		}, nil
	}

	byThread := groupByThread(input.Entries)
	client := mpv1.NewInferenceCoreClient(a.clients.InferenceCore)

	consolidated := make([]MemoryEntry, 0, len(grouped))
	modelled, skipped := 0, 0
	for _, group := range grouped {
		entries := byThread[group.ThreadID]
		if len(entries) == 0 || modelled >= maxConsolidationCalls {
			if modelled >= maxConsolidationCalls {
				skipped++
			}
			consolidated = append(consolidated, group)
			continue
		}
		content, err := a.consolidateThread(ctx, client, group, entries)
		modelled++
		switch {
		case err != nil:
			a.logger.Warn("memory consolidation fell back to the deterministic summary",
				"thread_id", group.ThreadID, "org_id", group.OrgID, "err", err)
			consolidated = append(consolidated, group)
		case content == "":
			// The model judged the thread not worth remembering. Emitting an
			// empty row would overwrite real memory with nothing, so drop the
			// group instead of consolidating it to emptiness.
			a.logger.Info("thread has no durable memory to keep",
				"thread_id", group.ThreadID, "org_id", group.OrgID)
		default:
			group.Content = content
			consolidated = append(consolidated, group)
		}
	}

	summary := fmt.Sprintf(
		"consolidated %d entries into %d summaries (%d model-consolidated",
		len(input.Entries), len(consolidated), modelled)
	if skipped > 0 {
		summary += fmt.Sprintf(", %d over the %d-call cap kept deterministic",
			skipped, maxConsolidationCalls)
	}
	summary += ")"
	return ConsolidationOutput{ConsolidatedEntries: consolidated, Summary: summary}, nil
}

// consolidateThread runs one thread's consolidation. Returns "" when the model
// reports nothing durable.
func (a *Activities) consolidateThread(
	ctx context.Context,
	client mpv1.InferenceCoreClient,
	group MemoryEntry,
	entries []MemoryEntry,
) (string, error) {
	transcript := renderEntries(entries)
	if strings.TrimSpace(transcript) == "" {
		return "", nil
	}

	callCtx, cancel := context.WithTimeout(ctx, consolidationTimeout)
	defer cancel()

	resp, err := client.Infer(callCtx, &mpv1.InferRequest{
		RequestId: fmt.Sprintf("memory-consolidation-%s", group.ThreadID),
		OrgId:     group.OrgID,
		Model:     MemoryConsolidationModel,
		Messages: []*mpv1.ChatMessage{
			{Role: "user", Content: consolidationPrompt(transcript)},
		},
		Temperature: consolidationTemperature,
		MaxTokens:   consolidationMaxTokens,
		// Not ZDR: the entry gate already required an attested durable posture
		// for this workflow, and the consolidated result is written back to
		// durable memory. Declaring ZDR here would ask inference-core to refuse
		// the very persistence this activity exists to produce.
		Zdr: false,
	})
	if err != nil {
		if status.Code(err) == codes.Unavailable {
			return "", fmt.Errorf("inference-core unavailable: %w", err)
		}
		return "", err
	}

	content := strings.TrimSpace(resp.GetContent())
	if content == "" {
		// An empty completion is not "nothing to remember" — it is also what a
		// failed or truncated call returns. Treat it as a failure so the
		// deterministic summary is kept rather than the thread being dropped.
		return "", fmt.Errorf("model returned an empty consolidation")
	}
	if strings.EqualFold(content, consolidationNothing) {
		return "", nil
	}
	return content, nil
}

// groupByThread indexes entries the same way summarizeMemoryEntries groups them,
// so a group and its source entries always line up.
func groupByThread(entries []MemoryEntry) map[string][]MemoryEntry {
	byThread := make(map[string][]MemoryEntry)
	for _, entry := range entries {
		threadID := entry.ThreadID
		if threadID == "" {
			// Dropped, matching summarizeMemoryEntries. The doc comment above is
			// load-bearing: these two groupings MUST agree, so a placeholder
			// bucket here while the other drops would silently attribute a
			// blended summary to entries that were never summarised.
			//
			// See the long note at the sibling site for why a shared "unknown"
			// bucket was a cross-user memory blender waiting on a working index.
			continue
		}
		byThread[threadID] = append(byThread[threadID], entry)
	}
	return byThread
}

// renderEntries lays out one thread's entries oldest-first for the prompt.
//
// Order matters: the prompt asks the model to supersede stale statements with
// newer ones, which it can only do if "newer" is visible in the input.
func renderEntries(entries []MemoryEntry) string {
	ordered := make([]MemoryEntry, len(entries))
	copy(ordered, entries)
	for i := 1; i < len(ordered); i++ {
		for j := i; j > 0 && ordered[j].CreatedAt.Before(ordered[j-1].CreatedAt); j-- {
			ordered[j], ordered[j-1] = ordered[j-1], ordered[j]
		}
	}
	var out strings.Builder
	for _, entry := range ordered {
		content := strings.TrimSpace(entry.Content)
		if content == "" {
			continue
		}
		if !entry.CreatedAt.IsZero() {
			out.WriteString(entry.CreatedAt.UTC().Format("2006-01-02"))
			out.WriteString(": ")
		}
		out.WriteString(content)
		out.WriteString("\n")
	}
	return out.String()
}
