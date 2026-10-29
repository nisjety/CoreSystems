package http

import "testing"

func TestUpdateConversationIDKeepsAPersonalBoundedSet(t *testing.T) {
	initial := []string{"conv_b", "conv_a", "conv_b", ""}
	pinned := updateConversationID(initial, "conv_c", true)
	if got, want := len(pinned), 3; got != want {
		t.Fatalf("pin count = %d, want %d", got, want)
	}
	if pinned[0] != "conv_c" || pinned[1] != "conv_b" || pinned[2] != "conv_a" {
		t.Fatalf("pin order = %#v, want newest preference first", pinned)
	}

	unpinned := updateConversationID(pinned, "conv_b", false)
	if got, want := len(unpinned), 2; got != want {
		t.Fatalf("unpin count = %d, want %d", got, want)
	}
	for _, id := range unpinned {
		if id == "conv_b" {
			t.Fatalf("unpin retained %q in %#v", id, unpinned)
		}
	}
}
