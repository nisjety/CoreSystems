package activities

import (
	"context"
	"io"
	"log/slog"
	"strings"
	"testing"
	"time"
)

// quietLogger keeps the fallback WARNs out of test output. Defined here rather
// than reused from integration_test.go, which is in the external `activities_test`
// package and therefore invisible to these internal tests.
func quietLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(io.Discard, nil))
}

func at(day int) time.Time {
	return time.Date(2026, 7, day, 12, 0, 0, 0, time.UTC)
}

// The regression that made this file exist. `summary[:512]` sliced by BYTE, so a
// Norwegian æ/ø/å landing on the boundary was cut in half and invalid UTF-8 was
// written into durable memory.
func TestTruncateRunesNeverSplitsAMultiByteRune(t *testing.T) {
	// One ASCII byte then 600 two-byte runes. The offset matters: with an even
	// cap and only two-byte runes, a byte slice at 512 always lands ON a
	// boundary, so the bug would hide. The leading "x" shifts every rune by one
	// byte and puts byte 512 in the MIDDLE of a rune, which is the real case —
	// any ASCII in the text (a digit, a space, punctuation) creates it.
	long := "x" + strings.Repeat("æ", 600)

	// First: the old behaviour really was broken. This is the bug, not a
	// hypothetical.
	if utf8Valid(long[:deterministicSummaryRunes]) {
		t.Fatal("the byte slice did not split a rune, so this test proves nothing " +
			"— pick an input where byte 512 falls mid-rune")
	}

	got := truncateRunes(long, deterministicSummaryRunes)
	if !utf8Valid(got) {
		t.Fatal("truncated output is not valid UTF-8")
	}
	if n := len([]rune(got)); n != deterministicSummaryRunes {
		t.Fatalf("kept %d runes, want %d", n, deterministicSummaryRunes)
	}
}

func utf8Valid(s string) bool {
	for _, r := range s {
		if r == '�' {
			return false
		}
	}
	return true
}

func TestTruncateRunesLeavesShortStringsAlone(t *testing.T) {
	for _, s := range []string{"", "kort", strings.Repeat("a", deterministicSummaryRunes)} {
		if got := truncateRunes(s, deterministicSummaryRunes); got != s {
			t.Fatalf("truncateRunes(%q) = %q, want it unchanged", s, got)
		}
	}
}

// The prompt asks the model to supersede stale statements with newer ones, which
// it can only do if the input is ordered.
func TestRenderEntriesIsOldestFirst(t *testing.T) {
	rendered := renderEntries([]MemoryEntry{
		{Content: "newest", CreatedAt: at(3)},
		{Content: "oldest", CreatedAt: at(1)},
		{Content: "middle", CreatedAt: at(2)},
	})
	oldest := strings.Index(rendered, "oldest")
	middle := strings.Index(rendered, "middle")
	newest := strings.Index(rendered, "newest")
	if !(oldest < middle && middle < newest) {
		t.Fatalf("entries are not oldest-first:\n%s", rendered)
	}
}

func TestRenderEntriesDropsBlanksAndStampsDates(t *testing.T) {
	rendered := renderEntries([]MemoryEntry{
		{Content: "   ", CreatedAt: at(1)},
		{Content: "kunden heter Aquatiq", CreatedAt: at(2)},
	})
	if strings.Count(rendered, "\n") != 1 {
		t.Fatalf("blank entry was not dropped:\n%q", rendered)
	}
	if !strings.Contains(rendered, "2026-07-02: kunden heter Aquatiq") {
		t.Fatalf("missing date stamp:\n%q", rendered)
	}
}

// A zero CreatedAt must not render as a fake date.
func TestRenderEntriesOmitsAZeroTimestamp(t *testing.T) {
	rendered := renderEntries([]MemoryEntry{{Content: "no timestamp"}})
	if strings.Contains(rendered, "0001-01-01") || strings.Contains(rendered, ":") {
		t.Fatalf("zero time rendered as a date: %q", rendered)
	}
}

// The grouping the model path relies on must match the deterministic grouping, or
// a group and its source entries would not line up and threads would be
// consolidated from the wrong content.
func TestGroupByThreadMatchesTheDeterministicGrouping(t *testing.T) {
	entries := []MemoryEntry{
		{ThreadID: "t1", OrgID: "org", Content: "a", CreatedAt: at(1)},
		{ThreadID: "t2", OrgID: "org", Content: "b", CreatedAt: at(1)},
		{ThreadID: "", OrgID: "org", Content: "c", CreatedAt: at(1)},
		{ThreadID: "t1", OrgID: "org", Content: "d", CreatedAt: at(2)},
	}
	grouped := summarizeMemoryEntries(entries)
	byThread := groupByThread(entries)

	if len(grouped) != len(byThread) {
		t.Fatalf("%d groups vs %d thread buckets", len(grouped), len(byThread))
	}
	for _, g := range grouped {
		if len(byThread[g.ThreadID]) == 0 {
			t.Fatalf("group %q has no source entries; the two groupings disagree", g.ThreadID)
		}
	}
	// An unattributable entry is DROPPED by both groupings, not bucketed under a
	// shared placeholder. This assertion is inverted from what it was: the old
	// "unknown" bucket collapsed every thread-less memory from every USER in the
	// org into one prompt and wrote the blended result back as a single row owned
	// by nobody. The search that feeds this is org-wide, so that was a cross-user
	// memory blender waiting for the vector index to start returning rows.
	if _, present := byThread["unknown"]; present {
		t.Fatalf("a blank thread id must be dropped, not bucketed: %v", byThread)
	}
	if len(byThread) != 2 {
		t.Fatalf("expected only the two real threads, got %v", byThread)
	}
	for _, g := range grouped {
		if g.ThreadID == "" || g.ThreadID == "unknown" {
			t.Fatalf("summarize produced an unattributable group: %q", g.ThreadID)
		}
	}
}

func TestSummarizeMemoryEntriesKeepsTheNewestTimestampPerThread(t *testing.T) {
	grouped := summarizeMemoryEntries([]MemoryEntry{
		{ThreadID: "t1", OrgID: "org", Content: "a", CreatedAt: at(1)},
		{ThreadID: "t1", OrgID: "org", Content: "b", CreatedAt: at(5)},
		{ThreadID: "t1", OrgID: "org", Content: "c", CreatedAt: at(3)},
	})
	if len(grouped) != 1 {
		t.Fatalf("want 1 group, got %d", len(grouped))
	}
	if !grouped[0].CreatedAt.Equal(at(5)) {
		t.Fatalf("CreatedAt = %v, want the newest entry's %v", grouped[0].CreatedAt, at(5))
	}
}

// No entries must not produce a model call or an empty row.
func TestSummarizeMemoryActivityWithNoEntriesDoesNothing(t *testing.T) {
	a := NewActivities(quietLogger(), nil)
	out, err := a.SummarizeMemoryActivity(context.Background(), ConsolidationInput{})
	if err != nil {
		t.Fatalf("err = %v", err)
	}
	if len(out.ConsolidatedEntries) != 0 {
		t.Fatalf("entries = %v, want none", out.ConsolidatedEntries)
	}
	if out.Summary != "no entries to consolidate" {
		t.Fatalf("summary = %q", out.Summary)
	}
}

// An unreachable inference-core degrades to the deterministic summary and says so
// — it must never fail the workflow, because a sweep that returns the old
// behaviour beats one that errors and retries the whole batch.
func TestSummarizeMemoryActivityFallsBackWhenInferenceIsUnavailable(t *testing.T) {
	a := NewActivities(quietLogger(), nil)
	out, err := a.SummarizeMemoryActivity(context.Background(), ConsolidationInput{
		Entries: []MemoryEntry{
			{ThreadID: "t1", OrgID: "org", Content: "kunden heter Aquatiq", CreatedAt: at(1)},
		},
	})
	if err != nil {
		t.Fatalf("err = %v, want a graceful fallback", err)
	}
	if len(out.ConsolidatedEntries) != 1 {
		t.Fatalf("want the deterministic group, got %v", out.ConsolidatedEntries)
	}
	if !strings.Contains(out.Summary, "deterministic") {
		t.Fatalf("summary = %q, must admit it did not consolidate", out.Summary)
	}
}

// The model id must stay a concrete model. A `velion-*` intent tier is re-routed
// by prompt size and lands a fixed-token micro-call on a reasoning model that
// answers 200 with an empty body.
func TestConsolidationModelIsNotAnIntentTier(t *testing.T) {
	if strings.HasPrefix(MemoryConsolidationModel, "velion-") {
		t.Fatalf("MemoryConsolidationModel = %q, must be a concrete model id",
			MemoryConsolidationModel)
	}
}

// The prompt must forbid invention: a summary that smooths over gaps produces
// confident memories the conversation never supported, and those then ground
// future answers.
func TestConsolidationPromptForbidsInference(t *testing.T) {
	p := consolidationPrompt("entry")
	for _, want := range []string{"Do not infer", "ONLY what the entries state", consolidationNothing} {
		if !strings.Contains(p, want) {
			t.Fatalf("prompt is missing %q", want)
		}
	}
	if !strings.Contains(p, "entry") {
		t.Fatal("prompt does not include the transcript")
	}
}
