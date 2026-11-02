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
		"ResolveSharedThreadReadDecisionEvidence",
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
		"fenceActiveSpaces",
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
	// Register replay keeps an archived room archived through the constant,
	// never through a literal that this scan would miss.
	register := funcs["Register"]
	if !strings.Contains(register, "RegistrationStateArchived") {
		t.Error("Register must reference RegistrationStateArchived when it keeps a room archived")
	}
}

// The gateway reaches the readable resolver only from the two read routes
// (the ?readable=1 membership read and the room-state decision helper).
func TestReadableResolverIsOnlyUsedByTheReadRoutes(t *testing.T) {
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
	if strings.Join(users, ",") != "space_room_state.go,spaces.go" {
		t.Fatalf("ResolveReadableUserMembership is used by %v", users)
	}
}
