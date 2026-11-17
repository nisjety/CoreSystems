package sessionreview

import (
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// WS12-S4: a deleted turn has no content left, so the reviewer's transcript skips it along with its tombstone, and the
// turns on either side render as before.
func TestRenderTranscript_SkipsDeletedTurns(t *testing.T) {
	out := RenderTranscript([]*mpv1.SessionMessage{
		{Role: "user", Content: "how do I cache?"},
		{Role: "user", Content: "a question the author deleted", RevisionState: "deleted"},
		{Role: "assistant", Content: "use the LRU"},
	})
	want := "USER: how do I cache?\n\nASSISTANT: use the LRU"
	if out != want {
		t.Fatalf("transcript mismatch:\n got: %q\nwant: %q", out, want)
	}
}
