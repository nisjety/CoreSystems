package spaces

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// WS12-S1: a direct-message roster is fixed at registration. This source scan
// finds every function that INSERTs, UPDATEs or DELETEs space_memberships and
// requires each to be one of: the registration/purge paths that legitimately
// write a DM roster, or a writer that refuses a DM (directly, or through the
// shared prelude that does). A new membership writer fails here until it is
// classified, so nobody can quietly widen a DM.

var membershipWriteStmt = regexp.MustCompile(`(?is)(insert\s+into|update|delete\s+from)\s+space_memberships\b`)

// goFunctionsOf returns each top-level function of one file, comment lines removed.
func goFunctionsOf(t *testing.T, file string) map[string]string {
	t.Helper()
	raw, err := os.ReadFile(file)
	if err != nil {
		t.Fatalf("read %s: %v", file, err)
	}
	src := strings.ReplaceAll(string(raw), "\r\n", "\n")
	starts := repositoryFuncStart.FindAllStringSubmatchIndex(src, -1)
	out := make(map[string]string, len(starts))
	for i, m := range starts {
		end := len(src)
		if i+1 < len(starts) {
			end = starts[i+1][0]
		}
		var code []string
		for _, line := range strings.Split(src[m[0]:end], "\n") {
			if strings.HasPrefix(strings.TrimSpace(line), "//") {
				continue
			}
			code = append(code, line)
		}
		out[src[m[2]:m[3]]] = strings.Join(code, "\n")
	}
	return out
}

func TestEveryMembershipWriterRefusesOrIsAKnownDMPath(t *testing.T) {
	// writers that may legitimately touch a DM roster.
	registrationPaths := map[string]string{
		"Register":                     "seeds the fixed roster inside the registration transaction",
		"purgeControlSpaceProjections": "deletes every row when the Space is deleted",
	}
	// writers that must refuse a DM, and the text that proves it.
	refusals := map[string][]string{
		"replaceMemberships": {"IsDM()", "ErrFixedRoster"},
		"JoinSpace":          {"space.Kind != KindRoom"},
		// These three share roomOpPrelude, which answers 409 room_fixed_roster.
		"AddMember":        {"beginRoomOp("},
		"ChangeMemberRole": {"beginRoomOp("},
		"RemoveMember":     {"beginRoomOp("},
	}

	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	var found []string
	for _, file := range files {
		if strings.HasSuffix(file, "_test.go") {
			continue
		}
		for name, body := range goFunctionsOf(t, file) {
			if !membershipWriteStmt.MatchString(body) {
				continue
			}
			found = append(found, name)
			if _, ok := registrationPaths[name]; ok {
				continue
			}
			proof, ok := refusals[name]
			if !ok {
				t.Errorf("%s (%s) writes space_memberships but is neither a registration path nor a known DM-refusing writer; add a DM guard and list it here", name, file)
				continue
			}
			for _, needle := range proof {
				if !strings.Contains(body, needle) {
					t.Errorf("%s no longer contains %q: the DM guard is gone", name, needle)
				}
			}
		}
	}
	sort.Strings(found)
	for name := range refusals {
		if sort.SearchStrings(found, name) >= len(found) || found[sort.SearchStrings(found, name)] != name {
			t.Errorf("listed writer %s no longer writes space_memberships; update this scan", name)
		}
	}

	// The shared prelude is the guard the three member operations rely on.
	prelude := goFunctionsOf(t, "membership_ops.go")["roomOpPrelude"]
	if !strings.Contains(prelude, "IsDM()") || !strings.Contains(prelude, "CodeRoomFixedRoster") {
		t.Error("roomOpPrelude must refuse a DM with CodeRoomFixedRoster")
	}
	// Room settings and archive operations refuse a DM as well.
	if archive := goFunctionsOf(t, "archive.go")["setArchiveState"]; !strings.Contains(archive, "IsDM()") {
		t.Error("setArchiveState must refuse a DM")
	}
}
