package zdr

import (
	"encoding/json"
	"strings"
	"testing"
	"testing/fstest"
)

// The text-version minimum (ZDR-Z-12-13 step 2): the oldest text version whose acknowledgement still
// counts. Absent means the current version; a minimum that is not a listed version, or that is above the
// current one, is refused at load; versions compare by the integer after the leading "v".

// withIndex returns the embedded wording with its index rewritten by edit.
func withIndex(t *testing.T, edit func(index map[string]any)) fstest.MapFS {
	t.Helper()
	files := copyEmbeddedWording(t)
	var index map[string]any
	if err := json.Unmarshal(files[wordingIndexPath].Data, &index); err != nil {
		t.Fatal(err)
	}
	edit(index)
	raw, err := json.Marshal(index)
	if err != nil {
		t.Fatal(err)
	}
	files[wordingIndexPath] = &fstest.MapFile{Data: raw}
	return files
}

func versionsOf(index map[string]any) map[string]any {
	return index["versions"].(map[string]any)
}

func TestTheShippedWordingSetsItsMinimumToItsCurrentVersion(t *testing.T) {
	wording := loadDefaultWordingForTest(t)
	if wording.MinimumVersion() != wording.Version() {
		t.Fatalf("shipped minimum %q, current %q: until a new text ships, every acknowledgement of the current text counts", wording.MinimumVersion(), wording.Version())
	}
}

func TestAnAbsentMinimumMeansTheCurrentVersion(t *testing.T) {
	assessor, err := Default()
	if err != nil {
		t.Fatal(err)
	}
	files := withIndex(t, func(index map[string]any) { delete(index, "minimum") })
	wording, err := loadWordingFrom(files, assessor)
	if err != nil {
		t.Fatalf("an index without a minimum must load: %v", err)
	}
	if wording.MinimumVersion() != wording.Version() {
		t.Fatalf("minimum %q with none set, want the current version %q", wording.MinimumVersion(), wording.Version())
	}
}

func TestTheLoaderRefusesAMinimumItCannotHonour(t *testing.T) {
	assessor, err := Default()
	if err != nil {
		t.Fatal(err)
	}
	cases := map[string]func(index map[string]any){
		"a minimum that is not a listed version": func(index map[string]any) { index["minimum"] = "v0" },
		"a minimum above the current version": func(index map[string]any) {
			versionsOf(index)["v2"] = map[string]any{"status": "draft", "files": map[string]any{}}
			index["minimum"] = "v2"
		},
		"a version key that is not v<digits>": func(index map[string]any) {
			versionsOf(index)["v1-rc"] = map[string]any{"status": "draft", "files": map[string]any{}}
		},
		"a minimum that is not v<digits>": func(index map[string]any) {
			versionsOf(index)["V1"] = map[string]any{"status": "draft", "files": map[string]any{}}
			index["minimum"] = "V1"
		},
	}
	for name, edit := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := loadWordingFrom(withIndex(t, edit), assessor); err == nil {
				t.Fatalf("the loader accepted %s", name)
			}
		})
	}
}

// A minimum below the current version loads, and the comparison is numeric: v10 is above v9.
func TestAMinimumBelowTheCurrentVersionLoadsAndComparesNumerically(t *testing.T) {
	assessor, err := Default()
	if err != nil {
		t.Fatal(err)
	}
	files := withIndex(t, func(index map[string]any) {
		versions := versionsOf(index)
		current := versions["v1"].(map[string]any)
		pins := map[string]any{}
		for name, pin := range current["files"].(map[string]any) {
			pins["v10/"+strings.TrimPrefix(name, "v1/")] = pin
		}
		versions["v10"] = map[string]any{"status": "draft", "files": pins}
		versions["v9"] = map[string]any{"status": "superseded", "files": map[string]any{}}
		index["current"] = "v10"
		index["minimum"] = "v9"
	})
	for name, file := range copyEmbeddedWording(t) {
		if strings.HasPrefix(name, wordingRoot+"/v1/") {
			files[wordingRoot+"/v10/"+strings.TrimPrefix(name, wordingRoot+"/v1/")] = file
		}
	}
	wording, err := loadWordingFrom(files, assessor)
	if err != nil {
		t.Fatalf("current v10 with minimum v9 must load (numeric, not string, order): %v", err)
	}
	if wording.Version() != "v10" || wording.MinimumVersion() != "v9" {
		t.Fatalf("loaded %s with minimum %s, want v10 and v9", wording.Version(), wording.MinimumVersion())
	}
}

func TestTextVersionAtLeastComparesTheNumberAfterV(t *testing.T) {
	cases := []struct {
		version, minimum string
		want             bool
	}{
		{"v1", "v1", true},
		{"v2", "v1", true},
		{"v1", "v2", false},
		{"v10", "v9", true},
		{"v9", "v10", false},
		{"v01", "v1", true},
		{"", "v1", false},
		{"v1", "", false},
		{"1", "v1", false},
		{"v", "v1", false},
		{"v1a", "v1", false},
		{"V2", "v1", false},
		{"v-1", "v1", false},
		{"v2", "v+1", false},
		{"v99999999999999999999999", "v1", false},
	}
	for _, c := range cases {
		if got := TextVersionAtLeast(c.version, c.minimum); got != c.want {
			t.Errorf("TextVersionAtLeast(%q, %q) = %v, want %v", c.version, c.minimum, got, c.want)
		}
	}
}
