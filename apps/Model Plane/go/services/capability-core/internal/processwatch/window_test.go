package processwatch

import (
	"context"
	"testing"
	"time"

	"google.golang.org/protobuf/types/known/timestamppb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// D18: the instants and the revision a member's history window places a watched process by.

func capturedAt(c *mpv1.ProcessOutputChunk, at time.Time) *mpv1.ProcessOutputChunk {
	c.CapturedAt = timestamppb.New(at)
	return c
}

var windowBase = time.Date(2026, 11, 18, 9, 30, 0, 0, time.UTC)

// A member's history window places a line by the instant the source first recorded its bytes.
func TestALineCarriesTheInstantItsSourceCapturedIt(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		capturedAt(out(1, "one\ntwo\n"), windowBase),
		capturedAt(out(2, "three\n"), windowBase.Add(time.Minute)),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	want := []time.Time{windowBase, windowBase, windowBase.Add(time.Minute)}
	if len(got.lines) != len(want) {
		t.Fatalf("got %d lines, want %d", len(got.lines), len(want))
	}
	for i, line := range got.lines {
		if !line.CapturedAt.Equal(want[i]) {
			t.Fatalf("line %q captured at %v, want %v", line.Text, line.CapturedAt, want[i])
		}
	}
}

// A line that began before a window and ended inside it is not shown as if it began inside. It carries the
// EARLIEST instant of its bytes, and the next line begins fresh in the chunk that completed the first.
func TestALineSpanningChunksCarriesTheInstantItBegan(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		capturedAt(out(1, "ERR"), windowBase),
		capturedAt(out(2, "OR: undefined symbol\nnext\ntail"), windowBase.Add(time.Hour)),
		capturedAt(out(3, " end\n"), windowBase.Add(2*time.Hour)),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 3 {
		t.Fatalf("lines = %+v, want three", got.lines)
	}
	cases := []struct {
		text string
		at   time.Time
	}{
		{"ERROR: undefined symbol", windowBase},
		{"next", windowBase.Add(time.Hour)},
		// The tail opened in chunk 2 and was closed by chunk 3: it began in chunk 2.
		{"tail end", windowBase.Add(time.Hour)},
	}
	for i, want := range cases {
		if got.lines[i].Text != want.text || !got.lines[i].CapturedAt.Equal(want.at) {
			t.Fatalf("line %d = %q at %v, want %q at %v", i, got.lines[i].Text, got.lines[i].CapturedAt, want.text, want.at)
		}
	}
}

// Streams are tracked apart, so a stderr chunk never moves the instant a stdout fragment began at.
func TestEachStreamKeepsTheInstantItsOwnFragmentBegan(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		capturedAt(out(1, "std"), windowBase),
		capturedAt(errs(2, "problem\n"), windowBase.Add(time.Hour)),
		capturedAt(out(3, "out\n"), windowBase.Add(2*time.Hour)),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	byStream := map[string]time.Time{}
	for _, line := range got.lines {
		byStream[line.Stream] = line.CapturedAt
	}
	if !byStream[streamStdout].Equal(windowBase) {
		t.Fatalf("the stdout line began at %v, want %v", byStream[streamStdout], windowBase)
	}
	if !byStream[streamStderr].Equal(windowBase.Add(time.Hour)) {
		t.Fatalf("the stderr line began at %v, want %v", byStream[streamStderr], windowBase.Add(time.Hour))
	}
}

// A source that sets no instant leaves the zero time, which a history window cannot place and so hides. An
// unset protobuf timestamp must not read as the Unix epoch, which would place the line at the start of time.
func TestAChunkWithNoInstantLeavesTheLineUnplaced(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{out(1, "no instant\n")}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 1 || !got.lines[0].CapturedAt.IsZero() {
		t.Fatalf("lines = %+v, want one line with the zero instant", got.lines)
	}
	epoch := capturedAt(out(1, "epoch\n"), time.Unix(0, 0))
	got, err = assembleLines([]*mpv1.ProcessOutputChunk{epoch}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if !got.lines[0].CapturedAt.IsZero() {
		t.Fatalf("an epoch timestamp placed a line at %v; it is an unset one", got.lines[0].CapturedAt)
	}
}

// The final unterminated output of a drained source is a line too, and carries when it began.
func TestADrainedFragmentCarriesTheInstantItBegan(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		capturedAt(out(1, "last words"), windowBase),
		capturedAt(out(2, " of a failing build"), windowBase.Add(time.Hour)),
	}, 0, true)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 1 || got.lines[0].Text != "last words of a failing build" {
		t.Fatalf("lines = %+v, want the one drained line", got.lines)
	}
	if !got.lines[0].CapturedAt.Equal(windowBase) {
		t.Fatalf("the drained line began at %v, want %v", got.lines[0].CapturedAt, windowBase)
	}
}

// What a window places the source by: the revision the process was registered under and the instant it ended.
// An unset end is the zero time, which a window hides.
func TestPollCarriesWhatAHistoryWindowPlacesTheSourceBy(t *testing.T) {
	t.Parallel()
	endedAt := time.Date(2026, 11, 18, 10, 0, 0, 0, time.UTC)
	exited := liveProcess()
	exited.State = mpv1.ProcessState_PROCESS_STATE_EXITED
	exited.RecipientAudienceRevision = 7
	exited.EndedAt = timestamppb.New(endedAt)
	registry := &fakeRegistry{process: exited, page: &mpv1.ReadProcessOutputResponse{NextCursor: 9}}

	result, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if result.SourceRevision != 7 {
		t.Fatalf("source revision = %d, want the process's own 7", result.SourceRevision)
	}
	if !result.EndedAt.Equal(endedAt) {
		t.Fatalf("ended at %v, want %v", result.EndedAt, endedAt)
	}

	// A process that never says when it ended, and one that carries no revision, leave the zero values.
	registry = &fakeRegistry{process: liveProcess()}
	result, err = newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if result.SourceRevision != 0 || !result.EndedAt.IsZero() {
		t.Fatalf("revision = %d, ended at %v; an unset value must stay unset, not read as a real one", result.SourceRevision, result.EndedAt)
	}
}
