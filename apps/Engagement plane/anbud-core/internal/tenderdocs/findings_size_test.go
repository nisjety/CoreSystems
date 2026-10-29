package tenderdocs

import (
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/docstore"
)

// A finding is a few kilobytes at the most, and an upload keeps as many as four hundred: with the longest path and the
// longest sentence, in characters that take many bytes once they are encoded, they come to more than the store holds,
// and the upload was answered with a failure that the log calls a bug, for an archive of a few kilobytes. It is kept
// with fewer findings, as it is when there are more of them than an upload keeps, and says how many it left out.
func TestAnUploadWhoseFindingsAreMoreThanTheStoreHoldsIsKeptWithFewer(t *testing.T) {
	f := newFixture(t)
	letter := string(rune(0x1D538)) // a letter that takes four bytes
	word := strings.Repeat(letter, 9)
	// A name of line separators, which JSON writes in six bytes each: the longest a path can be once it is encoded.
	name := strings.Repeat(string(rune(0x2028)), 236) + ".txt"

	var text strings.Builder
	for _, certificate := range []string{"iso 14001", "iso 9001", "miljøfyrtårn", "startbank", "achilles", "skatteattest", "hms-egenerklæring", "espd"} {
		for i := 0; i < 50; i++ {
			text.WriteString("Leverandøren skal ha " + certificate + strings.Repeat(" "+word, 55) + "\n\n")
		}
	}
	up, created, err := f.upload(t, zipOf(t, file(name, text.String())))
	if err != nil || !created {
		t.Fatalf("Upload: created %v, err %v: an archive that has too much to say is kept with less of it, and not refused", created, err)
	}
	if len(up.Findings) == 0 || len(up.Findings) >= docstore.MaxFindings {
		t.Fatalf("%d findings kept: some, and fewer than the %d that were found", len(up.Findings), docstore.MaxFindings)
	}
	if got := len(up.Findings) + up.FindingsDropped; got != docstore.MaxFindings {
		t.Errorf("%d kept and %d left out make %d, want %d: nothing is lost without a number", len(up.Findings), up.FindingsDropped, got, docstore.MaxFindings)
	}
	_, findings, err := f.store.creates[0].Encoded()
	if err != nil || len(findings) > docstore.MaxEncodedBytes {
		t.Errorf("what was stored does not fit what the store holds: %d bytes, %v", len(findings), err)
	}
	if strings.Contains(f.logs.String(), "a bug") {
		t.Errorf("an archive that is only large is logged as a bug: %s", f.logs.String())
	}
}
