package normalize

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
)

// TestNormaliseCapturedNoticesCountsOnly normalises every captured notice the parser reads. The
// notices are gitignored (they hold contact persons), so this runs only where they exist. It prints
// counts and fixed text only: never a value, an identifier or a file's contents.
func TestNormaliseCapturedNoticesCountsOnly(t *testing.T) {
	files, err := filepath.Glob("../../testdata/captured/*.xml")
	if err != nil {
		t.Fatal(err)
	}
	if len(files) == 0 {
		t.Skip("no captured notices")
	}
	var normalised, withDeadline int
	rules := map[string]int{}
	problems := map[string]int{}
	for _, file := range files {
		data, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("%s: %v", filepath.Base(file), err)
		}
		n, err := eforms.Parse(data)
		if err != nil {
			continue
		}
		normalised++
		r := Notice(n)
		if r.EarliestDeadline != nil {
			withDeadline++
		}
		for _, c := range r.Changes {
			rules[c.Rule]++
		}
		for _, p := range r.Problems {
			problems[p]++
		}
	}
	t.Logf("normalised %d notices, %d with a deadline", normalised, withDeadline)
	t.Logf("changes by rule: %v", rules)
	t.Logf("problems: %v", problems)
}
