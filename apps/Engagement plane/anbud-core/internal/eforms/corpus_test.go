package eforms

import (
	"os"
	"path/filepath"
	"testing"
)

// TestSniffAcceptsEveryCapturedNotice runs the sniffer over the real notices
// captured by cmd/doffin-probe. They are gitignored (licence unconfirmed, and
// they contain contact persons' personal data), so this test only runs on a
// machine that has them. It prints file names only, never content.
func TestSniffAcceptsEveryCapturedNotice(t *testing.T) {
	files, err := filepath.Glob("../../testdata/captured/*.xml")
	if err != nil {
		t.Fatal(err)
	}
	if len(files) == 0 {
		t.Skip("no captured notices; run: go run ./cmd/doffin-probe -discover=false -samples 12 -save-dir testdata/captured")
	}

	roots := map[string]bool{"ContractNotice": true, "ContractAwardNotice": true, "PriorInformationNotice": true}
	for _, file := range files {
		name := filepath.Base(file)
		data, err := os.ReadFile(file)
		if err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		h, err := Sniff(data)
		if err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		if h.NoticeID == "" || h.VersionID == "" || h.SDKVersion == "" || h.NoticeTypeCode == "" || h.SubTypeCode == "" {
			t.Errorf("%s: an identifying field is empty: %+v", name, h)
		}
		if !roots[h.Root] {
			t.Errorf("%s: unexpected root element %q", name, h.Root)
		}
		if !h.Flavour.Known() {
			t.Errorf("%s: flavour %q is not one this code knows", name, h.Flavour)
		}
	}
	t.Logf("sniffed %d captured notices", len(files))
}
