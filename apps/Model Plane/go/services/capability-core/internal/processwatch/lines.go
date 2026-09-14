// Package processwatch is S4.3's first watch source: a background process's
// output, read from sandbox-manager's S4.2 registry.
//
// # Why this polls instead of consuming events
//
// The adoption plan prefers owner events and permits polling "only behind an
// adapter with explicit lag/rate/budget behavior". This polls, because S4.2
// already built the better mechanism: the registry has host-assigned monotonic
// cursors, `(process_id, seq)` idempotency, `gap_before`, `retained_from_seq`,
// and a terminal state on every row. `ReadProcessOutput(after_seq)` is therefore
// idempotent by construction, which is exactly what "crash before cursor commit"
// needs — re-read and there is nothing to reconcile.
//
// An event per output chunk would be a firehose (the host flushes every 250ms
// per stream, per process), would need its own deduplication and its own
// retention, and sandbox-manager publishes nothing to NATS today — so the event
// route needs a publisher, a stream and a provisioner change before the adapter
// could be written at all.
//
// The budget the plan asks for lives in `watch.PollInterval` and
// [MaxReadBytes]; the honest cost is that a watch is not a live tail.
package processwatch

import (
	"bytes"
	"fmt"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/watch"
)

// MaxReadBytes is one poll's read budget. The cursor carries over, so a busy
// process is drained across polls rather than in one unbounded read.
const MaxReadBytes int64 = 32 * 1024

// MaxPendingBytes caps an unterminated line held between polls.
//
// S4.2's host already flushes a line longer than 64 KiB with
// `ends_with_newline = false` and records that residual risk rather than hiding
// it. This is the reader's half of the same bound: without it, a program that
// writes megabytes with no newline would stall the cursor forever — the watch
// would never consume anything and never make progress. At the cap the pending
// text is treated as a line, which is the same compromise the writer already
// made.
const MaxPendingBytes = 64 * 1024

// stream names, matching the registry's enum and the predicate vocabulary.
const (
	streamStdout = "stdout"
	streamStderr = "stderr"
	// streamSystem is the registry's own marker stream (S4.2 writes a
	// "host lost" note there). It is NOT the process's output and is dropped
	// before predicates ever see it — see the SYSTEM branch in assembleLines.
	streamSystem = "system"
)

func streamName(stream mpv1.ProcessStream) string {
	switch stream {
	case mpv1.ProcessStream_PROCESS_STREAM_STDOUT:
		return streamStdout
	case mpv1.ProcessStream_PROCESS_STREAM_STDERR:
		return streamStderr
	case mpv1.ProcessStream_PROCESS_STREAM_SYSTEM:
		return streamSystem
	default:
		return ""
	}
}

// assembled is the result of turning one page of chunks into complete lines.
type assembled struct {
	// Lines that are COMPLETE — every one ended with a newline, or the source
	// is terminal and this is its final unterminated output.
	lines []watch.Line
	// Cursor that is safe to commit: the highest chunk seq after which NO
	// stream still holds an unterminated fragment. See [assembleLines].
	cursor int64
}

// assembleLines turns a page of output chunks into complete lines.
//
// # Why "complete" is load-bearing
//
// A predicate evaluated against a partial line fires on a prefix that the next
// chunk completes into something else. That is the same class of error as
// scrubbing a secret cut in half, which is precisely why S4.2's host scrubs at
// line boundaries and puts `ends_with_newline` on every chunk. This function is
// where that guarantee is converted into something a predicate can use.
//
// # Why the cursor can lag the chunks
//
// A chunk's lines are attributed to that chunk's seq, and the committed cursor
// must never move past a chunk whose stream still has an unterminated fragment
// — because chunks at or below the cursor are never re-read, and the fragment's
// continuation arrives later. Streams are tracked SEPARATELY: stdout and stderr
// interleave in one seq sequence, and joining a stdout fragment to a stderr
// chunk would fabricate a line neither stream ever emitted.
//
// So the safe cursor is the highest seq at which every stream is "clean". If
// one stream keeps a fragment open, the cursor stalls there and the rest of the
// page is re-read next poll — correct, and bounded by [MaxPendingBytes] so it
// cannot stall forever.
//
// # Terminal sources
//
// When the process has ended and been drained, whatever remains unterminated IS
// the final output and is emitted as a line. Dropping it would lose the last
// thing a failing program said, which is usually why it failed.
func assembleLines(chunks []*mpv1.ProcessOutputChunk, afterSeq int64, sourceTerminal bool) (assembled, error) {
	result := assembled{cursor: afterSeq}
	pending := map[string][]byte{}
	// A chunk seq is only safe once every stream is clean at that point.
	clean := func() bool {
		for _, fragment := range pending {
			if len(fragment) > 0 {
				return false
			}
		}
		return true
	}

	for _, chunk := range chunks {
		if chunk == nil {
			continue
		}
		name := streamName(chunk.GetStream())
		if name == "" {
			// An unrecognized stream is not silently folded into stdout: a
			// stream selector that matched content from a stream it did not
			// name would be a filter that does not filter.
			return assembled{}, fmt.Errorf("process output chunk %d has an unrecognized stream", chunk.GetSeq())
		}
		if chunk.GetSeq() <= result.cursor && chunk.GetSeq() <= afterSeq {
			// Defensive: the registry reads strictly after the cursor, so this
			// should not happen. Re-consuming it would re-emit lines the watch
			// already saw.
			continue
		}
		if name == streamSystem {
			// The registry's own marker, not the process's output. Dropped
			// before any predicate sees it, because an `any` watch firing on
			// "host lost" would report the REGISTRY as the thing that spoke —
			// and a `contains` watch could be matched by text the process never
			// wrote. Nothing is lost: what the marker means is already carried
			// by the process's state, which this adapter reports as terminal.
			//
			// The cursor still advances past it, so a marker cannot stall a
			// watch.
			if clean() {
				result.cursor = chunk.GetSeq()
			}
			continue
		}

		buffer := append(pending[name], chunk.GetContent()...)
		for {
			index := bytes.IndexByte(buffer, '\n')
			if index < 0 {
				break
			}
			result.lines = append(result.lines, watch.Line{
				Cursor: chunk.GetSeq(),
				Stream: name,
				Text:   string(trimCR(buffer[:index])),
			})
			buffer = buffer[index+1:]
		}
		if len(buffer) > MaxPendingBytes {
			// The bound described on MaxPendingBytes: treat it as a line rather
			// than stall the cursor forever.
			result.lines = append(result.lines, watch.Line{
				Cursor: chunk.GetSeq(),
				Stream: name,
				Text:   string(trimCR(buffer[:MaxPendingBytes])),
			})
			buffer = buffer[MaxPendingBytes:]
		}
		pending[name] = buffer
		if clean() {
			result.cursor = chunk.GetSeq()
		}
	}

	if sourceTerminal {
		// Drain: whatever is left is the final output, and every chunk read is
		// consumed because nothing more will ever arrive.
		for _, chunk := range chunks {
			if chunk != nil && chunk.GetSeq() > result.cursor {
				result.cursor = chunk.GetSeq()
			}
		}
		for _, name := range []string{streamStdout, streamStderr} {
			fragment := pending[name]
			if len(fragment) == 0 {
				continue
			}
			result.lines = append(result.lines, watch.Line{
				Cursor: result.cursor,
				Stream: name,
				Text:   string(trimCR(fragment)),
			})
		}
	}
	return result, nil
}

// trimCR drops a single trailing carriage return, so a program writing CRLF
// does not leave one on the end of every matched line.
func trimCR(line []byte) []byte {
	if len(line) > 0 && line[len(line)-1] == '\r' {
		return line[:len(line)-1]
	}
	return line
}
