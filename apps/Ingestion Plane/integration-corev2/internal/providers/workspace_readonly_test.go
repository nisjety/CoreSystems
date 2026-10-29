package providers

import (
	"slices"
	"testing"
)

func TestWorkspaceReadOnlyIncludesReportingWithoutWriteScopes(t *testing.T) {
	google := GoogleWorkspace()
	caps := ResolveCapabilities(google, nil, []string{"workspace_readonly"})
	scopes := ResolveScopes(google, caps)
	for _, scope := range []string{"https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/analytics.readonly", "https://www.googleapis.com/auth/drive.readonly", "https://www.googleapis.com/auth/gmail.readonly"} {
		if !slices.Contains(scopes, scope) {
			t.Fatalf("missing %s", scope)
		}
	}
	for _, cap := range []string{"gmail.send", "ads.manage"} {
		if slices.Contains(caps, cap) {
			t.Fatalf("unexpected write capability %s", cap)
		}
	}
	microsoft := Microsoft()
	caps = ResolveCapabilities(microsoft, nil, []string{"workspace_readonly"})
	if !slices.Contains(caps, "mail.read") || slices.Contains(caps, "mail.send") {
		t.Fatal("Microsoft workspace must read without sending mail")
	}
}
