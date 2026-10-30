package match

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
)

// TestRequirementRulesOnCapturedNoticesCountsOnly reports how many selection criteria of
// the captured notices the default rules recognise. The notices are gitignored, so this runs only
// where they exist. It prints counts and capability names only, never criterion text.
func TestRequirementRulesOnCapturedNoticesCountsOnly(t *testing.T) {
	files, err := filepath.Glob("../../testdata/captured/*.xml")
	if err != nil {
		t.Fatal(err)
	}
	if len(files) == 0 {
		t.Skip("no captured notices")
	}
	var requirements, recognised, parsed, pointToDocuments int
	byCapability := map[Capability]int{}
	byType := map[string]int{}
	for _, file := range files {
		data, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("%s: %v", filepath.Base(file), err)
		}
		n, err := eforms.Parse(data)
		if err != nil {
			continue
		}
		parsed++
		for _, req := range n.Requirements {
			if req.Type == "epo-procurement-document" {
				pointToDocuments++
				break
			}
		}
		for _, req := range n.SelectionCriteria {
			requirements++
			byType[req.Type]++
			if c, ok := capabilityOf(req.Name + "\n" + req.Description); ok {
				recognised++
				byCapability[c]++
			}
		}
	}
	t.Logf("selection criteria %d, recognised by the default rules %d", requirements, recognised)
	t.Logf("by capability: %v", byCapability)
	t.Logf("parsed notices %d, of which %d say the criteria are in the tender documents", parsed, pointToDocuments)
	t.Logf("by selection-criterion code: %v", byType)
}
