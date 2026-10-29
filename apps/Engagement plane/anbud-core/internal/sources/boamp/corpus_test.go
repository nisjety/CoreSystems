package boamp

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
)

// TestCorpusConvertsAndParses reads a directory of BOAMP search responses (BOAMP_CORPUS_DIR, a day or
// more of captured pages) and checks every eForms record: the conversion writes it, and the eForms
// parser reads the result. It is skipped unless the directory is named, because the corpus is a live
// capture, not a fixture.
func TestCorpusConvertsAndParses(t *testing.T) {
	dir := os.Getenv("BOAMP_CORPUS_DIR")
	if dir == "" {
		t.Skip("set BOAMP_CORPUS_DIR to a directory of captured BOAMP search responses")
	}
	pages, err := filepath.Glob(filepath.Join(dir, "*.json"))
	if err != nil || len(pages) == 0 {
		t.Fatalf("no captured pages in %s: %v", dir, err)
	}
	var eForms, converted, parsed, sdk112, other int
	for _, page := range pages {
		raw, err := os.ReadFile(page)
		if err != nil {
			t.Fatal(err)
		}
		var decoded recordsResponse
		if err := json.Unmarshal(raw, &decoded); err != nil {
			t.Fatalf("%s: %v", page, err)
		}
		for _, r := range decoded.Results {
			if r.Donnees == "" {
				continue
			}
			body, err := eFormsXML([]byte(r.Donnees))
			if errors.Is(err, errNotEForms) {
				other++
				continue
			}
			eForms++
			if err != nil {
				t.Errorf("%s: %s did not convert: %v", page, r.Idweb, err)
				continue
			}
			converted++
			notice, err := eforms.Parse(body)
			switch {
			case err == nil:
				parsed++
			case errors.Is(err, eforms.ErrNoFieldMap) && notice.SDKVersion == "1.12":
				// The parser has no field map for SDK 1.12 yet (docs/SOURCES.md): the notice is kept and
				// shown as unsupported, the same as any notice the parser does not read.
				sdk112++
			default:
				t.Errorf("%s: %s converted but the eForms parser refused it: %v", page, r.Idweb, err)
			}
		}
	}
	t.Logf("eForms records %d, converted %d, parsed %d, SDK 1.12 without a field map %d; not eForms %d", eForms, converted, parsed, sdk112, other)
	if eForms == 0 {
		t.Fatal("the corpus holds no eForms record")
	}
}
