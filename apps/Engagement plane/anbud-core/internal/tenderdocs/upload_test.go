package tenderdocs

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/docstore"
)

// A realistic package: a text, a Word file and a PDF, each with something the rules know and a
// sentence they do not.
func tenderPackage(t *testing.T) []byte {
	return zipOf(t,
		file("konkurransegrunnlag.txt", "Kapittel 1 Om oppdraget\n\nLeverandøren skal ha ISO 9001 eller tilsvarende.\n\nHEMMELIG-SETNING-777 står bare her."),
		entry{name: "vedlegg/krav.docx", data: docx(t, "Leverandøren må være registrert i StartBANK.", "HEMMELIG-SETNING-888 i Word.")},
		entry{name: "vedlegg/kontrakt.pdf", data: miniPDF("Tildelingskriterier: poeng for ISO 14001.")},
	)
}

func TestUploadReadsTheDocumentsAndStoresTheManifestAndTheFindings(t *testing.T) {
	f := newFixture(t)
	archive := tenderPackage(t)

	up, created, err := f.upload(t, archive)
	if err != nil || !created {
		t.Fatalf("Upload = created %v, err %v", created, err)
	}
	if len(f.store.creates) != 1 {
		t.Fatalf("stored %d uploads, want 1", len(f.store.creates))
	}
	in := f.store.creates[0]
	if in.OrgID != testOrg || in.NoticeID != testNotice || in.UploadedBy != testUser || in.ArchiveName != "konkurransegrunnlag.zip" ||
		in.ArchiveSHA256 != hashOf(archive) || in.ArchiveBytes != int64(len(archive)) || in.Retention != 90*24*time.Hour {
		t.Errorf("stored %+v", in)
	}
	if up.ID == 0 {
		t.Errorf("the upload that was stored is not returned: %+v", up)
	}

	docs := byPath(in.Documents)
	for path, kind := range map[string]string{"konkurransegrunnlag.txt": "text", "vedlegg/krav.docx": "docx", "vedlegg/kontrakt.pdf": "pdf"} {
		d, ok := docs[path]
		if !ok || d.Status != docstore.StatusRead || d.Kind != kind || d.SHA256 == "" || d.Size <= 0 || d.Chars <= 0 || d.Extractor != kind {
			t.Errorf("%s: %+v", path, d)
		}
	}
	if docs["vedlegg/kontrakt.pdf"].Pages != 1 {
		t.Errorf("the PDF has %d pages, want 1", docs["vedlegg/kontrakt.pdf"].Pages)
	}

	found := capabilities(in.Findings)
	if got := found["iso9001"]; len(got) != 1 || got[0].Class != "requirement" || got[0].Document != "konkurransegrunnlag.txt" || got[0].Paragraph == 0 || got[0].Page != 0 ||
		!strings.Contains(got[0].Snippet, "ISO 9001") {
		t.Errorf("iso9001: %+v", got)
	}
	if got := found["startbank"]; len(got) != 1 || got[0].Class != "requirement" || got[0].Document != "vedlegg/krav.docx" {
		t.Errorf("startbank: %+v", got)
	}
	if got := found["iso14001"]; len(got) != 1 || got[0].Class != "award" || got[0].Document != "vedlegg/kontrakt.pdf" || got[0].Page != 1 || got[0].Paragraph != 0 {
		t.Errorf("iso14001: %+v", got)
	}
}

// The text of a document is read and dropped. What is kept is the manifest and the sentences that
// name something the rules know: a sentence that names nothing is in no table, no log and no response.
func TestUploadKeepsNoTextAndNoBytes(t *testing.T) {
	f := newFixture(t)
	if _, _, err := f.upload(t, tenderPackage(t)); err != nil {
		t.Fatal(err)
	}
	stored, err := json.Marshal(f.store.creates[0])
	if err != nil {
		t.Fatal(err)
	}
	for _, secret := range []string{"HEMMELIG-SETNING-777", "HEMMELIG-SETNING-888", "Kapittel 1 Om oppdraget"} {
		if strings.Contains(string(stored), secret) {
			t.Errorf("%q was stored", secret)
		}
		if strings.Contains(f.logs.String(), secret) {
			t.Errorf("%q was logged", secret)
		}
	}
	// A sentence that does name something is the finding, and nothing more of the document comes with it.
	if !strings.Contains(string(stored), "Leverandøren skal ha ISO 9001 eller tilsvarende.") {
		t.Errorf("the finding's sentence is not stored: %s", stored)
	}
}

func TestUploadLogsCountsAndNeverNamesOrSentences(t *testing.T) {
	f := newFixture(t)
	if _, _, err := f.upload(t, tenderPackage(t)); err != nil {
		t.Fatal(err)
	}
	logged := f.logs.String()
	if logged == "" {
		t.Fatal("an upload leaves nothing in the log")
	}
	for _, private := range []string{"konkurransegrunnlag", "krav.docx", "kontrakt.pdf", "ISO 9001", "StartBANK", testUser, testOrg, "vedlegg"} {
		if strings.Contains(logged, private) {
			t.Errorf("the log names %q: %s", private, logged)
		}
	}
	for _, want := range []string{`"files":3`, `"read":3`, `"findings":3`} {
		if !strings.Contains(logged, want) {
			t.Errorf("the log lacks %s: %s", want, logged)
		}
	}
}

// What cannot be read is in the manifest, with its status, so that nobody takes an unread file for a
// file with nothing to find.
func TestUploadSaysWhatItCouldNotRead(t *testing.T) {
	f := newFixture(t)
	archive := zipOf(t,
		file("lest.txt", "Leverandøren skal ha ISO 9001."),
		entry{name: "skann.pdf", data: miniPDF("")},                        // a scan: a PDF with no text at all
		entry{name: "bilde.png", data: []byte("\x89PNG\r\n\x1a\n.......")}, // a picture: not read
		entry{name: "odelagt.pdf", data: []byte("%PDF-1.4\nthis is not a pdf at all")},
	)
	if _, _, err := f.upload(t, archive); err != nil {
		t.Fatal(err)
	}
	docs := byPath(f.store.creates[0].Documents)
	want := map[string]struct{ status, kind, reason string }{
		"lest.txt":    {docstore.StatusRead, "text", ""},
		"skann.pdf":   {docstore.StatusEmpty, "pdf", ""},
		"bilde.png":   {docstore.StatusUnsupported, "image", ""},
		"odelagt.pdf": {docstore.StatusFailed, "pdf", "damaged"},
	}
	for path, w := range want {
		d, ok := docs[path]
		if !ok || d.Status != w.status || d.Kind != w.kind || d.Reason != w.reason {
			t.Errorf("%s = %+v, want status %s kind %s reason %q", path, d, w.status, w.kind, w.reason)
		}
	}
}

func TestUploadListsTheFilesItSkippedWithTheirReason(t *testing.T) {
	f := newFixture(t, func(o *Options) { o.Limits.MaxDepth = 2 })
	inner := zipOf(t, file("dypt.txt", "ISO 9001"))
	middle := zipOf(t, entry{name: "inner.zip", data: inner})
	archive := zipOf(t,
		file("grei.txt", "Leverandøren skal ha ISO 9001."),
		file("../utenfor.txt", "x"),
		entry{name: "vedlegg.zip", data: middle},
		file("", "uten navn"),
	)
	if _, _, err := f.upload(t, archive); err != nil {
		t.Fatal(err)
	}
	docs := byPath(f.store.creates[0].Documents)
	if d := docs["grei.txt"]; d.Status != docstore.StatusRead {
		t.Errorf("grei.txt = %+v", d)
	}
	skipped := 0
	reasons := map[string]bool{}
	for _, d := range f.store.creates[0].Documents {
		if d.Status != docstore.StatusSkipped {
			continue
		}
		skipped++
		reasons[d.Reason] = true
		if d.SHA256 != "" || d.Path == "" || d.Kind == "" {
			t.Errorf("a skipped file is listed as %+v: it has no hash, and it has a path and a kind", d)
		}
	}
	if skipped != 3 || !reasons["bad_name"] || !reasons["too_deep"] {
		t.Errorf("skipped %d with reasons %v; want ../utenfor.txt, the nameless one and the archive that is too deep", skipped, reasons)
	}
	if _, ok := docs["(unnamed)"]; !ok {
		t.Errorf("a file with no name is not listed as (unnamed): %v", f.store.creates[0].Documents)
	}
}

// The same bytes under two names are read once. The second is listed, as a duplicate, so that the
// manifest still shows every file the archive held.
func TestUploadReadsTheSameBytesOnce(t *testing.T) {
	reader := &scriptReader{}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	reader.fn = readsAs(1)
	archive := zipOf(t, file("a.txt", "samme innhold"), file("kopi/b.txt", "samme innhold"), file("c.txt", "noe annet"))
	if _, _, err := f.upload(t, archive); err != nil {
		t.Fatal(err)
	}
	if reader.count() != 2 {
		t.Errorf("read %d files, want 2: the copy is not read again", reader.count())
	}
	docs := byPath(f.store.creates[0].Documents)
	if docs["a.txt"].Status != docstore.StatusRead || docs["kopi/b.txt"].Status != docstore.StatusDuplicate || docs["c.txt"].Status != docstore.StatusRead {
		t.Errorf("manifest = %+v", f.store.creates[0].Documents)
	}
	if docs["kopi/b.txt"].SHA256 != docs["a.txt"].SHA256 {
		t.Errorf("the duplicate has another hash")
	}
}

// Uploading the same archive again is the same upload: it is not read again, and it is not stored again.
func TestUploadingTheSameArchiveAgainReadsNothing(t *testing.T) {
	reader := &scriptReader{}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	reader.fn = readsAs(1)
	archive := zipOf(t, file("a.txt", "innhold"))

	first, created, err := f.upload(t, archive)
	if err != nil || !created {
		t.Fatalf("first = %v, %v", created, err)
	}
	reads := reader.count()
	again, created, err := f.upload(t, archive)
	if err != nil || created || again.ID != first.ID {
		t.Fatalf("second = id %d created %v err %v; want the first upload and not created", again.ID, created, err)
	}
	if reader.count() != reads {
		t.Errorf("the second upload read %d more files", reader.count()-reads)
	}
	if len(f.store.creates) != 1 {
		t.Errorf("stored %d uploads, want 1", len(f.store.creates))
	}
}

// Another archive for the same notice is another upload, read and stored on its own: a revision of the
// documents does not replace the first, and the evidence is what the two say together.
func TestADifferentArchiveForTheSameNoticeIsAnotherUpload(t *testing.T) {
	f := newFixture(t)
	first, created, err := f.upload(t, zipOf(t, file("a.txt", "Leverandøren skal ha ISO 9001.")))
	if err != nil || !created {
		t.Fatalf("first = %v, %v", created, err)
	}
	second, created, err := f.upload(t, zipOf(t, file("b.txt", "Leverandøren må være registrert i StartBANK.")))
	if err != nil || !created || second.ID == first.ID {
		t.Fatalf("second = id %d created %v err %v; want a new upload", second.ID, created, err)
	}
	if len(f.store.creates) != 2 || f.store.creates[0].ArchiveSHA256 == f.store.creates[1].ArchiveSHA256 {
		t.Errorf("stored %d uploads", len(f.store.creates))
	}
	if got := capabilities(f.store.creates[1].Findings); len(got["startbank"]) != 1 || len(got["iso9001"]) != 0 {
		t.Errorf("the second upload's findings are %+v: they must be its own", f.store.creates[1].Findings)
	}
}

// A hundred mentions of one certificate must not push everything else out, and the ones that say it is
// asked for are kept before the ones that only mention it. What is left out is counted.
func TestUploadKeepsTheFindingsThatMatterAndCountsTheRest(t *testing.T) {
	f := newFixture(t)
	var text []string
	for i := 0; i < 60; i++ {
		text = append(text, "ISO 9001 nevnes i dette avsnittet, og det er alt.")
	}
	for i := 0; i < 60; i++ {
		text = append(text, "Leverandøren skal ha ISO 9001 sertifisering.")
	}
	text = append(text, "Leverandøren må være registrert i StartBANK.")
	archive := zipOf(t, file("stor.txt", strings.Join(text, "\n\n")))
	if _, _, err := f.upload(t, archive); err != nil {
		t.Fatal(err)
	}
	in := f.store.creates[0]
	found := capabilities(in.Findings)
	if len(found["iso9001"]) != perCapability {
		t.Fatalf("kept %d findings of iso9001, want %d", len(found["iso9001"]), perCapability)
	}
	for _, fd := range found["iso9001"] {
		if fd.Class != "requirement" {
			t.Errorf("a %s finding was kept while requirements were left out", fd.Class)
		}
	}
	// Of equal weight the earliest are kept: the requirements are paragraphs 61 to 120, and the first 50 stay.
	if kept := found["iso9001"]; kept[0].Paragraph != 61 || kept[len(kept)-1].Paragraph != 60+perCapability {
		t.Errorf("kept paragraphs %d to %d, want 61 to %d", kept[0].Paragraph, kept[len(kept)-1].Paragraph, 60+perCapability)
	}
	if len(found["startbank"]) != 1 {
		t.Errorf("startbank was pushed out: %+v", found["startbank"])
	}
	if want := 120 - perCapability; in.FindingsDropped != want {
		t.Errorf("dropped %d, want %d", in.FindingsDropped, want)
	}
	// Kept in the order of the document, not in the order of importance.
	for i := 1; i < len(in.Findings); i++ {
		if in.Findings[i].Paragraph < in.Findings[i-1].Paragraph {
			t.Fatalf("findings are out of document order at %d: %d after %d", i, in.Findings[i].Paragraph, in.Findings[i-1].Paragraph)
		}
	}
}

func TestUploadCleansTheArchiveName(t *testing.T) {
	long := strings.Repeat("å", 300)
	for name, tc := range map[string]struct{ in, want string }{
		"plain":         {"Konkurransegrunnlag.zip", "Konkurransegrunnlag.zip"},
		"controls":      {"a\x00b\nc\td.zip", "abcd.zip"},
		"bidi override": {"a\u202eb.zip", "ab.zip"},
		"embeddings":    {"a\u202ab\u202c.zip", "ab.zip"},
		"override":      {"a\u202db.zip", "ab.zip"},
		"isolates":      {"a\u2066b\u2069.zip", "ab.zip"},
		"zero width":    {"a\u200bb\u200cc\u200dd.zip", "abcd.zip"},
		"soft hyphen":   {"a\u00adb.zip", "ab.zip"},
		"word joiner":   {"a\u2060b.zip", "ab.zip"},
		"byte order":    {"\ufeffa.zip", "a.zip"},
		// The rest of what cannot be seen, which the list of a few left in a name that is stored and shown.
		"direction marks":     {"a\u200eb\u200fc\u061cd.zip", "abcd.zip"},
		"invisible operators": {"a\u2061b\u2062c\u2063d\u2064e.zip", "abcde.zip"},
		"mongolian separator": {"a\u180eb.zip", "ab.zip"},
		"tag characters":      {"a\U000E0041\U000E0042b.zip", "ab.zip"},
		"variation selectors": {"a\ufe0fb\U000E0100c.zip", "abc.zip"},
		"fillers and joiners": {"a\u034fb\u3164c\uffa0d\u115fe\u1160f\u17b4g.zip", "abcdefg.zip"},
		"letters of a script": {"\ud55c\uae00-\u0645\u0631\u062d\u0628\u0627-\u6f22\u5b57.zip", "\ud55c\uae00-\u0645\u0631\u062d\u0628\u0627-\u6f22\u5b57.zip"},
		"spaces":              {"  Tilbud  ", "Tilbud"},
		"too long":            {long, strings.Repeat("å", maxArchiveNameRunes)},
		"invalid utf-8":       {"a\xffb.zip", "a�b.zip"},
	} {
		f := newFixture(t)
		_, _, err := f.svc.Upload(t.Context(), UploadRequest{OrgID: testOrg, NoticeID: testNotice, ArchiveName: tc.in, Archive: zipOf(t, file("a.txt", name))})
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if got := f.store.creates[0].ArchiveName; got != tc.want {
			t.Errorf("%s: name = %q, want %q", name, got, tc.want)
		}
	}
}
