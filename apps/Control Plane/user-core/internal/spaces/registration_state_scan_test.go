package spaces

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// WS04-S7: an archived room is a read-class state. Exactly the read
// surfaces below see it; every write-class resolver keeps
// registration_state='active'. This test reads repository.go as text, so a
// new resolver that quietly widens the state set, or a read surface that
// loses it, fails here without needing a database.

var repositoryFuncStart = regexp.MustCompile(`(?m)^func (?:\([^)]*\) )?(\w+)\(`)

// repositoryFunctions returns each top-level function of repository.go with
// its comment lines removed (a comment may mention "archived" freely).
func repositoryFunctions(t *testing.T) map[string]string {
	t.Helper()
	raw, err := os.ReadFile("repository.go")
	if err != nil {
		t.Fatalf("read repository.go: %v", err)
	}
	src := strings.ReplaceAll(string(raw), "\r\n", "\n")
	starts := repositoryFuncStart.FindAllStringSubmatchIndex(src, -1)
	if len(starts) == 0 {
		t.Fatal("no functions found in repository.go")
	}
	out := make(map[string]string, len(starts))
	for i, m := range starts {
		end := len(src)
		if i+1 < len(starts) {
			end = starts[i+1][0]
		}
		name := src[m[2]:m[3]]
		var code []string
		for _, line := range strings.Split(src[m[0]:end], "\n") {
			if strings.HasPrefix(strings.TrimSpace(line), "//") {
				continue
			}
			code = append(code, line)
		}
		out[name] = strings.Join(code, "\n")
	}
	return out
}

func TestOnlyTheReadSurfacesResolveAnArchivedRoom(t *testing.T) {
	funcs := repositoryFunctions(t)
	want := []string{
		"RosterForSpace",
		"SpacesForSubject",
		"ResolveReadableUserMembership",
		"resolveUserMembership",
		"ResolveSharedRetrievalDecisionEvidence",
		// WS09-S7: stopping a live run is allowed in an archived room (a run
		// started before the archive must still be stoppable).
		"ResolveSharedRunControlDecisionEvidence",
		// D21h (with D22aj): a starter may stop their own browser run in an
		// archived room, for the same reason.
		"ResolveSharedBrowserRunStopDecisionEvidence",
		"ResolveSharedThreadReadDecisionEvidence",
		// WS19-S2: a canvas read resolves an archived room; a canvas write takes
		// active only (pinned in TestArchivedStateSetsInTheReadSurfacesAreExact).
		"ResolveSharedCanvasDecisionEvidence",
		"fenceActiveSpaces",
	}
	var got []string
	for name, body := range funcs {
		if strings.Contains(body, "archived") {
			got = append(got, name)
		}
	}
	sort.Strings(got)
	sort.Strings(want)
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("functions that mention archived in repository.go:\n got  %v\n want %v", got, want)
	}
}

func TestArchivedStateSetsInTheReadSurfacesAreExact(t *testing.T) {
	funcs := repositoryFunctions(t)
	// The four SQL read surfaces and fenceActiveSpaces carry the widened set.
	for _, name := range []string{
		"RosterForSpace", "SpacesForSubject",
		"ResolveSharedRetrievalDecisionEvidence", "ResolveSharedThreadReadDecisionEvidence",
		"ResolveSharedRunControlDecisionEvidence", "ResolveSharedBrowserRunStopDecisionEvidence", "fenceActiveSpaces",
	} {
		body := funcs[name]
		if !regexp.MustCompile(`registration_state IN \('active',\s*'archived'\)`).MatchString(body) {
			t.Errorf("%s must read registration_state IN ('active','archived')", name)
		}
		if strings.Contains(body, "registration_state='active'") || strings.Contains(body, "registration_state = 'active'") {
			t.Errorf("%s must not keep an active-only registration_state filter", name)
		}
	}
	// resolveUserMembership takes its state set from the caller.
	resolver := funcs["resolveUserMembership"]
	if !strings.Contains(resolver, "registration_state = ANY($4::text[])") {
		t.Error("resolveUserMembership must filter registration_state with the caller's state set")
	}
	if strings.Contains(resolver, `"archived"`) || strings.Contains(resolver, "'archived'") {
		t.Error("resolveUserMembership must not hardcode the archived state; only ResolveReadableUserMembership passes it")
	}
	// The canvas resolver widens its read only: a write narrows the set to active.
	canvas := funcs["ResolveSharedCanvasDecisionEvidence"]
	if !strings.Contains(canvas, "registration_state = ANY($4::text[])") ||
		!regexp.MustCompile(`if write \{\s*states = \[\]string\{"active"\}\s*\}`).MatchString(canvas) {
		t.Error("ResolveSharedCanvasDecisionEvidence must resolve a canvas write in an active room only")
	}
}

func TestResolveUserMembershipCallersPassActiveOnlyExceptTheReadableOne(t *testing.T) {
	funcs := repositoryFunctions(t)
	callRe := regexp.MustCompile(`resolveUserMembership\(([^)]*)\)`)
	callers := map[string]string{}
	for name, body := range funcs {
		if name == "resolveUserMembership" {
			continue
		}
		for _, m := range callRe.FindAllStringSubmatch(body, -1) {
			callers[name] = m[1]
		}
	}
	if len(callers) != 2 {
		t.Fatalf("resolveUserMembership must have exactly two callers, got %v", callers)
	}
	if args := callers["ResolveCurrentUserMembership"]; !strings.Contains(args, `[]string{"active"}`) {
		t.Errorf("ResolveCurrentUserMembership must pass active only, got %q", args)
	}
	if args := callers["ResolveReadableUserMembership"]; !strings.Contains(args, `[]string{"active", "archived"}`) {
		t.Errorf("ResolveReadableUserMembership must pass active and archived, got %q", args)
	}
}

// Every write-class resolver and fence in repository.go keeps the active-only
// filter: nothing outside the read surfaces above may widen it.
func TestWriteClassResolversKeepTheActiveOnlyFilter(t *testing.T) {
	funcs := repositoryFunctions(t)
	for _, name := range []string{
		"resolvePersonalDecisionEvidence",
		"ResolveSharedThreadDecisionEvidence",
		"ResolveOwnScheduledThreadReadDecisionEvidence",
	} {
		body, ok := funcs[name]
		if !ok {
			t.Fatalf("%s not found in repository.go", name)
		}
		if !strings.Contains(body, "registration_state='active'") {
			t.Errorf("%s must keep registration_state='active'", name)
		}
		if strings.Contains(body, "archived") {
			t.Errorf("%s must not resolve an archived room", name)
		}
	}
	// A widened state filter (IN, ANY, <>, NOT IN) must name archived, so it is one
	// of the read surfaces pinned above; one that does not would let an archived
	// room through unnoticed.
	widened := regexp.MustCompile(`(?i)registration_state\s*(in\s*\(|=\s*any\s*\(|<>|!=|not\s+in\b)`)
	for name, body := range funcs {
		if widened.MatchString(body) && !strings.Contains(body, "archived") {
			t.Errorf("%s widens the registration_state filter without naming archived", name)
		}
	}
	// Register replay keeps an archived room archived through the constant,
	// never through a literal that this scan would miss.
	register := funcs["Register"]
	if !strings.Contains(register, "RegistrationStateArchived") {
		t.Error("Register must reference RegistrationStateArchived when it keeps a room archived")
	}
}

// The gateway reaches the readable resolver only from read routes. Each http
// file that takes it is listed with why that use is a read; a write route keeps
// the strict active-only resolver.
func TestReadableResolverIsOnlyUsedByTheReadRoutes(t *testing.T) {
	readRoutes := map[string]string{
		// Known gap left to the approval lane: revokeApprovalGrant and the policy
		// PUT (through elevatedMembership) are writes that take it as well.
		"approval_grants.go":            "WS10 rule list and GET approval policy read an archived room",
		"routes_space_kind_evidence.go": "W3-DEC-KIND signs only the kind of a Space the caller can read, no effect on it",
		"space_room_state.go":           "resolveReadKindEvidence serves the read decisions (retrieval, canvas read)",
		"spaces.go":                     "GET /spaces/{ref}/membership?readable=1 (WS04-S7)",
		"tool_policy.go":                "WS10-S5a GET tool policy; its PUT takes the active-only resolver",
	}
	httpDir := filepath.Join("..", "http")
	entries, err := os.ReadDir(httpDir)
	if err != nil {
		t.Fatalf("read http dir: %v", err)
	}
	var users []string
	for _, entry := range entries {
		name := entry.Name()
		if entry.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
			continue
		}
		raw, err := os.ReadFile(filepath.Join(httpDir, name))
		if err != nil {
			t.Fatalf("read %s: %v", name, err)
		}
		if strings.Contains(string(raw), "ResolveReadableUserMembership") {
			users = append(users, name)
		}
	}
	sort.Strings(users)
	want := make([]string, 0, len(readRoutes))
	for name := range readRoutes {
		want = append(want, name)
	}
	sort.Strings(want)
	if strings.Join(users, ",") != strings.Join(want, ",") {
		t.Fatalf("ResolveReadableUserMembership is used by %v; only the read routes %v may take it", users, want)
	}
}
