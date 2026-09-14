package processwatch

import (
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

func chunk(seq int64, stream mpv1.ProcessStream, text string) *mpv1.ProcessOutputChunk {
	return &mpv1.ProcessOutputChunk{
		Seq:             seq,
		Stream:          stream,
		Content:         []byte(text),
		EndsWithNewline: strings.HasSuffix(text, "\n"),
	}
}

func out(seq int64, text string) *mpv1.ProcessOutputChunk {
	return chunk(seq, mpv1.ProcessStream_PROCESS_STREAM_STDOUT, text)
}

func errs(seq int64, text string) *mpv1.ProcessOutputChunk {
	return chunk(seq, mpv1.ProcessStream_PROCESS_STREAM_STDERR, text)
}

func TestOneChunkSplitsIntoItsCompleteLines(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{out(1, "compiling\nlinking\ndone\n")}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 3 {
		t.Fatalf("got %d lines, want 3: %+v", len(got.lines), got.lines)
	}
	for _, line := range got.lines {
		if line.Cursor != 1 {
			t.Fatalf("a line is attributed to cursor %d, want its chunk's 1", line.Cursor)
		}
		if strings.Contains(line.Text, "\n") {
			t.Fatalf("a line still carries its newline: %q", line.Text)
		}
	}
	if got.cursor != 1 {
		t.Fatalf("cursor = %d, want 1 — the chunk ended cleanly", got.cursor)
	}
}

// The whole reason S4.2 carries ends_with_newline. A predicate evaluated
// against a prefix fires on text the next chunk completes into something else.
func TestAPartialLineIsHeldBackAndTheCursorStaysBehindIt(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		out(1, "first\n"),
		out(2, "ERR"),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 1 || got.lines[0].Text != "first" {
		t.Fatalf("lines = %+v, want only the complete one", got.lines)
	}
	if got.cursor != 1 {
		// Committing 2 would mean chunk 2 is never re-read and "ERR" is lost —
		// or worse, matched as if it were a whole line.
		t.Fatalf("cursor = %d, want 1 — it must not pass a chunk with an open fragment", got.cursor)
	}
}

// The completion arrives in a later chunk and the two halves join into ONE
// line, attributed to the chunk that completed it.
func TestAPartialLineJoinsItsContinuation(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		out(1, "ERR"),
		out(2, "OR: undefined symbol\n"),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 1 || got.lines[0].Text != "ERROR: undefined symbol" {
		t.Fatalf("lines = %+v, want one joined line", got.lines)
	}
	if got.lines[0].Cursor != 2 {
		t.Fatalf("the joined line is at cursor %d, want the chunk that completed it", got.lines[0].Cursor)
	}
	if got.cursor != 2 {
		t.Fatalf("cursor = %d, want 2 — everything is consumed", got.cursor)
	}
}

// Streams interleave in one seq sequence. Joining a stdout fragment to a stderr
// chunk would fabricate a line neither stream ever emitted — and, with a
// `contains` predicate, could manufacture a match out of two innocent halves.
func TestFragmentsNeverJoinAcrossStreams(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		out(1, "ERR"),
		errs(2, "OR: from stderr\n"),
		out(3, "OR: from stdout\n"),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 2 {
		t.Fatalf("got %d lines, want 2: %+v", len(got.lines), got.lines)
	}
	byStream := map[string]string{}
	for _, line := range got.lines {
		byStream[line.Stream] = line.Text
	}
	if byStream[streamStderr] != "OR: from stderr" {
		t.Fatalf("stderr line = %q; a stdout fragment leaked into it", byStream[streamStderr])
	}
	if byStream[streamStdout] != "ERROR: from stdout" {
		t.Fatalf("stdout line = %q, want the fragment joined to its OWN stream", byStream[streamStdout])
	}
}

// The cursor is the highest seq at which EVERY stream is clean. A stderr
// fragment opened at seq 1 holds the cursor down even though stdout is clean.
func TestTheCursorWaitsForEveryStreamToBeClean(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		errs(1, "partial"),
		out(2, "complete\n"),
		out(3, "also complete\n"),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if got.cursor != 0 {
		t.Fatalf("cursor = %d, want 0 — stderr has had an open fragment since seq 1", got.cursor)
	}
	// The stdout lines are still returned: they are complete, and withholding
	// them would delay a match for as long as the other stream stays open.
	if len(got.lines) != 2 {
		t.Fatalf("got %d lines, want the two complete stdout lines", len(got.lines))
	}
}

// A terminal, drained source: whatever is unterminated IS the final output.
// Dropping it loses the last thing a failing program said, which is usually why
// it failed.
func TestATerminalSourceFlushesItsFinalUnterminatedLine(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		out(1, "working\n"),
		errs(2, "fatal: no such file"),
	}, 0, true)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 2 {
		t.Fatalf("got %d lines, want the complete one and the final fragment: %+v", len(got.lines), got.lines)
	}
	if got.lines[1].Text != "fatal: no such file" || got.lines[1].Stream != streamStderr {
		t.Fatalf("final line = %+v", got.lines[1])
	}
	if got.cursor != 2 {
		t.Fatalf("cursor = %d, want 2 — a drained source consumes everything", got.cursor)
	}
}

// The registry's own marker stream is not the process speaking. An `any` watch
// firing on "host lost" would report the REGISTRY as the thing that spoke, and
// a `contains` watch could be matched by text the process never wrote.
func TestTheSystemStreamIsDroppedButStillAdvancesTheCursor(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		out(1, "working\n"),
		chunk(2, mpv1.ProcessStream_PROCESS_STREAM_SYSTEM, "host lost\n"),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 1 || got.lines[0].Text != "working" {
		t.Fatalf("lines = %+v, want only the process's own output", got.lines)
	}
	if got.cursor != 2 {
		// A marker must not stall a watch forever.
		t.Fatalf("cursor = %d, want 2 — the marker is consumed even though it is not surfaced", got.cursor)
	}
}

// A chunk with no stream is a bug upstream, not something to fold into stdout:
// a stream selector that matched content from a stream it did not name would be
// a filter that does not filter.
func TestAnUnrecognizedStreamIsAnError(t *testing.T) {
	t.Parallel()
	_, err := assembleLines([]*mpv1.ProcessOutputChunk{
		chunk(1, mpv1.ProcessStream_PROCESS_STREAM_UNSPECIFIED, "mystery\n"),
	}, 0, false)
	if err == nil {
		t.Fatal("a chunk with an unspecified stream was accepted")
	}
}

// Without this bound a program writing megabytes with no newline stalls the
// cursor forever: the watch would never consume anything and never progress.
// S4.2's host already made the same compromise on the writing side.
func TestAnEndlessFragmentIsCutAtTheBoundRatherThanStallingForever(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{
		out(1, strings.Repeat("x", MaxPendingBytes+10)),
	}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 1 {
		t.Fatalf("got %d lines, want the forced flush", len(got.lines))
	}
	if len(got.lines[0].Text) != MaxPendingBytes {
		t.Fatalf("flushed %d bytes, want the %d-byte bound", len(got.lines[0].Text), MaxPendingBytes)
	}
	// 10 bytes still pending, so the cursor stays behind the chunk.
	if got.cursor != 0 {
		t.Fatalf("cursor = %d, want 0 — a fragment remains", got.cursor)
	}
}

// A program writing CRLF must not leave a carriage return on the end of every
// matched line, where it would show up in a summary a person reads.
func TestCarriageReturnsAreTrimmed(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{out(1, "windows line\r\n")}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if got.lines[0].Text != "windows line" {
		t.Fatalf("line = %q, want the carriage return trimmed", got.lines[0].Text)
	}
}

// An empty page leaves everything where it was. A poll that read nothing must
// not move the cursor, or a later chunk at that seq would be skipped.
func TestAnEmptyPageLeavesTheCursorAlone(t *testing.T) {
	t.Parallel()
	got, err := assembleLines(nil, 17, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 0 || got.cursor != 17 {
		t.Fatalf("got %d lines at cursor %d, want none at 17", len(got.lines), got.cursor)
	}
}

// Blank lines are real output. A program that prints an empty line has said
// something, and an `any` watch should see it.
func TestBlankLinesAreLines(t *testing.T) {
	t.Parallel()
	got, err := assembleLines([]*mpv1.ProcessOutputChunk{out(1, "a\n\nb\n")}, 0, false)
	if err != nil {
		t.Fatalf("assembleLines: %v", err)
	}
	if len(got.lines) != 3 {
		t.Fatalf("got %d lines, want 3 including the blank: %+v", len(got.lines), got.lines)
	}
	if got.lines[1].Text != "" {
		t.Fatalf("the middle line is %q, want empty", got.lines[1].Text)
	}
}
