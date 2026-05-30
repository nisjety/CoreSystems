package workspace

import "testing"

func TestBuildWorkspaceIDIsDeterministic(t *testing.T) {
	left := buildWorkspaceID("org-123")
	right := buildWorkspaceID("org-123")

	if left != right {
		t.Fatalf("expected deterministic workspace id, got %q and %q", left, right)
	}
}

func TestBuildPersonalWorkspaceIDUsesAnonymousFallback(t *testing.T) {
	if got := buildPersonalWorkspaceID(""); got != "planner-anonymous" {
		t.Fatalf("expected anonymous fallback, got %q", got)
	}
}

func TestBuildPersonalWorkspaceIDIsUserScoped(t *testing.T) {
	got := buildPersonalWorkspaceID("user-123")
	if got == "planner-anonymous" || got == "planner" {
		t.Fatalf("expected user scoped workspace id, got %q", got)
	}
}
