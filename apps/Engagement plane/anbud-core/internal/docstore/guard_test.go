package docstore

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

// New(nil) has no pool. Any SQL would panic, so each refusal also proves that the store refused
// before it reached the database.

func TestCreateRefusesWhatTheDatabaseWouldRefuseAnyway(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("x", 241)
	cases := map[string]func(*NewUpload){
		"no organisation":                       func(u *NewUpload) { u.OrgID = "" },
		"an organisation with a space":          func(u *NewUpload) { u.OrgID = "org a" },
		"an organisation too long":              func(u *NewUpload) { u.OrgID = strings.Repeat("a", 129) },
		"not a notice id":                       func(u *NewUpload) { u.NoticeID = "2026-1" },
		"a notice id with SQL":                  func(u *NewUpload) { u.NoticeID = "2026-104837'; DROP TABLE x" },
		"a bad user":                            func(u *NewUpload) { u.UploadedBy = "Ola Nordmann" },
		"a short hash":                          func(u *NewUpload) { u.ArchiveSHA256 = "abc" },
		"an upper case hash":                    func(u *NewUpload) { u.ArchiveSHA256 = strings.ToUpper(sha("x")) },
		"no bytes":                              func(u *NewUpload) { u.ArchiveBytes = 0 },
		"an archive name too long":              func(u *NewUpload) { u.ArchiveName = strings.Repeat("n", 201) },
		"no retention":                          func(u *NewUpload) { u.Retention = 0 },
		"a negative retention":                  func(u *NewUpload) { u.Retention = -time.Hour },
		"three years of retention":              func(u *NewUpload) { u.Retention = 3 * 365 * 24 * time.Hour },
		"a negative limit for the notice":       func(u *NewUpload) { u.MaxPerNotice = -1 },
		"a negative limit for the organisation": func(u *NewUpload) { u.MaxPerOrg = -1 },
		"a retention under a second":            func(u *NewUpload) { u.Retention = MinRetention - time.Nanosecond },
		"a retention of a nanosecond":           func(u *NewUpload) { u.Retention = time.Nanosecond },
		"a document with no path":               func(u *NewUpload) { u.Documents[0].Path = "" },
		"a document path too long":              func(u *NewUpload) { u.Documents[0].Path = long },
		"a document with a bad hash":            func(u *NewUpload) { u.Documents[0].SHA256 = "nope" },
		"a document with an unknown status":     func(u *NewUpload) { u.Documents[0].Status = "maybe" },
		"a document with no kind":               func(u *NewUpload) { u.Documents[0].Kind = "" },
		"a document of negative size":           func(u *NewUpload) { u.Documents[0].Size = -1 },
		"too many documents":                    func(u *NewUpload) { u.Documents = make([]Document, MaxDocuments+1) },
		"a finding with no snippet":             func(u *NewUpload) { u.Findings[0].Snippet = "" },
		"a snippet too long":                    func(u *NewUpload) { u.Findings[0].Snippet = strings.Repeat("s", MaxSnippetRunes+1) },
		"a finding with an unknown class":       func(u *NewUpload) { u.Findings[0].Class = "maybe" },
		"a finding with no capability":          func(u *NewUpload) { u.Findings[0].Capability = "" },
		"a finding of no document":              func(u *NewUpload) { u.Findings[0].Document = "" },
		"a negative page":                       func(u *NewUpload) { u.Findings[0].Page = -1 },
		"too many findings":                     func(u *NewUpload) { u.Findings = make([]Finding, MaxFindings+1) },
	}
	for name, mutate := range cases {
		u := sample("org-a", "2026-100001", "a")
		mutate(&u)
		if _, _, err := New(nil).Create(context.Background(), u); !errors.Is(err, ErrInvalid) {
			t.Errorf("%s: err = %v, want ErrInvalid", name, err)
		}
	}
}

func TestReadsRefuseAnOrganisationOrNoticeThatIsNotOne(t *testing.T) {
	t.Parallel()

	s := New(nil)
	ctx := context.Background()
	if _, err := s.List(ctx, "", "2026-100001"); !errors.Is(err, ErrInvalid) {
		t.Errorf("List with no organisation: %v", err)
	}
	if _, err := s.List(ctx, "org-a", "x"); !errors.Is(err, ErrInvalid) {
		t.Errorf("List with a bad notice id: %v", err)
	}
	if _, err := s.EvidenceFor(ctx, "", []string{"2026-100001"}); !errors.Is(err, ErrInvalid) {
		t.Errorf("EvidenceFor with no organisation: %v", err)
	}
	if _, err := s.EvidenceFor(ctx, "org-a", []string{"2026-100001", "nope"}); !errors.Is(err, ErrInvalid) {
		t.Errorf("EvidenceFor with a bad notice id: %v", err)
	}
	ids := make([]string, MaxEvidenceNotices+1)
	for i := range ids {
		ids[i] = "2026-100001"
	}
	if _, err := s.EvidenceFor(ctx, "org-a", ids); !errors.Is(err, ErrInvalid) {
		t.Errorf("EvidenceFor with too many notices: %v", err)
	}
	if _, err := s.Delete(ctx, "org-a", "2026-100001", 0); !errors.Is(err, ErrInvalid) {
		t.Errorf("Delete of id 0: %v", err)
	}
	if _, err := s.Delete(ctx, "", "2026-100001", 5); !errors.Is(err, ErrInvalid) {
		t.Errorf("Delete with no organisation: %v", err)
	}
	if _, err := s.Delete(ctx, "org-a", "nope", 5); !errors.Is(err, ErrInvalid) {
		t.Errorf("Delete with a bad notice id: %v", err)
	}
	for name, call := range map[string]func() error{
		"no organisation": func() error { _, _, err := s.FindSame(ctx, "", "2026-100001", sha("x")); return err },
		"a bad notice id": func() error { _, _, err := s.FindSame(ctx, "org-a", "nope", sha("x")); return err },
		"a bad hash":      func() error { _, _, err := s.FindSame(ctx, "org-a", "2026-100001", "abc"); return err },
	} {
		if err := call(); !errors.Is(err, ErrInvalid) {
			t.Errorf("FindSame with %s: %v", name, err)
		}
	}
	if _, _, err := s.Counts(ctx, "", "2026-100001"); !errors.Is(err, ErrInvalid) {
		t.Errorf("Counts with no organisation: %v", err)
	}
}

func TestEvidenceForNothingAsksNothing(t *testing.T) {
	t.Parallel()

	got, err := New(nil).EvidenceFor(context.Background(), "org-a", nil)
	if err != nil || len(got) != 0 {
		t.Errorf("EvidenceFor of no notices = %v, %v; want an empty answer with no query", got, err)
	}
}

// What is stored is bounded by what the table accepts, and the store says so by name before it asks.
func TestWhatIsEncodedIsBoundedAndNamesTheFieldThatIsTooLarge(t *testing.T) {
	t.Parallel()

	ordinary := sample("org-a", "2026-100001", "a")
	ordinary.Findings = ordinary.Findings[:0]
	for len(ordinary.Findings) < MaxFindings {
		ordinary.Findings = append(ordinary.Findings, Finding{Document: strings.Repeat("d", 240), Capability: "iso9001", Class: "mention", Snippet: strings.Repeat("x", MaxSnippetRunes)})
	}
	if _, _, err := ordinary.Encoded(); err != nil {
		t.Errorf("400 findings of the longest ASCII text are refused: %v", err)
	}

	huge := sample("org-a", "2026-100001", "a")
	for len(huge.Findings) < MaxFindings {
		huge.Findings = append(huge.Findings, Finding{Document: strings.Repeat("\U0001F600", 240), Capability: "iso9001", Class: "mention", Snippet: strings.Repeat("\U0001F600", MaxSnippetRunes)})
	}
	if _, _, err := huge.Encoded(); !errors.Is(err, ErrInvalid) || !strings.Contains(err.Error(), "findings are too large") {
		t.Errorf("findings of four-byte characters at their longest: err = %v, want ErrInvalid naming the findings", err)
	}
	if _, _, err := New(nil).Create(context.Background(), huge); !errors.Is(err, ErrInvalid) {
		t.Errorf("Create of the same: %v", err)
	}

	manifest := sample("org-a", "2026-100001", "a")
	manifest.Documents = manifest.Documents[:0]
	for len(manifest.Documents) < MaxDocuments {
		manifest.Documents = append(manifest.Documents, Document{Path: strings.Repeat("\U0001F600", 240), Kind: "pdf", SHA256: sha("x"), Status: StatusRead, Extractor: strings.Repeat("e", 20), Reason: strings.Repeat("r", 40)})
	}
	if _, _, err := manifest.Encoded(); err != nil {
		t.Errorf("a manifest of 500 paths of the longest four-byte characters is refused: %v (the table accepts it)", err)
	}
}

// The characters < > and & are not written as \u003c and the like: a sentence made of them would be six
// times its length, in a size that is checked against what the database holds, where they are one byte.
func TestEncodedTextIsNotEscapedForHTML(t *testing.T) {
	t.Parallel()

	u := sample("org-a", "2026-100001", "a")
	u.Findings[0].Snippet = "a < b > c & d"
	_, findings, err := u.Encoded()
	if err != nil || !strings.Contains(string(findings), "a < b > c & d") {
		t.Errorf("findings = %s, %v", findings, err)
	}
}

// What is encoded is bounded to the byte: a manifest, or a list of findings, of exactly MaxEncodedBytes is
// stored, one byte more is refused, and the refusal says which of the two it was. The encoded text is the
// JSON and no more: it does not end in the newline the encoder writes.
func TestEncodedIsBoundedToTheByteAndSaysWhichPartIsTooLarge(t *testing.T) {
	t.Parallel()

	// Encoded does not look at what a path or a sentence says, only at how much it comes to: the checks of
	// their content are Create's.
	manifest := func(n int) NewUpload {
		u := sample("org-a", "2026-100001", "a")
		u.Documents = []Document{{Path: strings.Repeat("p", n), Kind: "pdf", SHA256: sha("x"), Status: StatusRead}}
		return u
	}
	findings := func(n int) NewUpload {
		u := sample("org-a", "2026-100001", "a")
		u.Findings = []Finding{{Document: "a.pdf", Capability: "iso9001", Class: "mention", Snippet: strings.Repeat("s", n)}}
		return u
	}

	for name, tc := range map[string]struct {
		build func(int) NewUpload
		part  func(documents, findings []byte) []byte
		said  string
	}{
		"the manifest": {manifest, func(d, _ []byte) []byte { return d }, "manifest is too large"},
		"the findings": {findings, func(_, f []byte) []byte { return f }, "findings are too large"},
	} {
		small, err := func() ([]byte, error) {
			d, f, err := tc.build(1).Encoded()
			return tc.part(d, f), err
		}()
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		exact := MaxEncodedBytes - len(small) + 1 // each more character of the text is one more byte
		d, f, err := tc.build(exact).Encoded()
		if err != nil {
			t.Fatalf("%s of exactly %d bytes is refused: %v", name, MaxEncodedBytes, err)
		}
		if got := tc.part(d, f); len(got) != MaxEncodedBytes || bytes.HasSuffix(got, []byte("\n")) {
			t.Errorf("%s: %d bytes, ending in a newline: %v; want exactly %d and no newline", name, len(got), bytes.HasSuffix(got, []byte("\n")), MaxEncodedBytes)
		}
		if _, _, err := tc.build(exact + 1).Encoded(); !errors.Is(err, ErrInvalid) || !strings.Contains(err.Error(), tc.said) {
			t.Errorf("%s of one byte over: err = %v, want ErrInvalid saying %q", name, err, tc.said)
		}
	}
}

// A table that holds an array is given an array: no files and no findings are [] and not null, which the
// table would refuse.
func TestAnUploadWithNothingInItIsEncodedAsEmptyLists(t *testing.T) {
	t.Parallel()

	u := sample("org-a", "2026-100001", "a")
	u.Documents, u.Findings = nil, nil
	documents, findings, err := u.Encoded()
	if err != nil || string(documents) != "[]" || string(findings) != "[]" {
		t.Errorf("Encoded = %q, %q, %v, want [] and []", documents, findings, err)
	}
}

// A producer that keeps the findings within MaxEncodedBytes adds up FindingBytes: the list of n findings is the
// brackets and every finding with a comma after it but the last, so it comes to 1 and what each of them is. It has to be
// right to the byte, for what is written out (quotes, backslashes, line separators, which JSON writes in six bytes, and
// letters of four bytes) and for what is not (the signs HTML cares for, which are not escaped).
func TestTheBytesOfTheFindingsAddUpToTheEncodedList(t *testing.T) {
	t.Parallel()

	findings := []Finding{
		{Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 2, Snippet: "Leverandøren skal ha ISO 9001."},
		{Document: "b/c.docx", Capability: "espd", Class: "mention", Paragraph: 14, Snippet: `a "quoted" word and a \ backslash`},
		{Document: strings.Repeat("\u2028", 20) + ".txt", Capability: "startbank", Class: "award", Paragraph: 1, Snippet: "a<b>&c " + string(rune(0x1D538))},
		{Document: "d.txt", Capability: "achilles", Class: "negated", Paragraph: 100000, Snippet: "x"},
	}
	for n := 1; n <= len(findings); n++ {
		_, encoded, err := NewUpload{Findings: findings[:n]}.Encoded()
		if err != nil {
			t.Fatalf("%d findings: %v", n, err)
		}
		sum := 1
		for _, f := range findings[:n] {
			size, err := FindingBytes(f)
			if err != nil {
				t.Fatalf("FindingBytes: %v", err)
			}
			sum += size
		}
		if sum != len(encoded) {
			t.Errorf("%d findings: the bytes of the findings add up to %d, and the list is %d bytes: %s", n, sum, len(encoded), encoded)
		}
	}
}

// The shortest and the longest retention are accepted, and what is outside them is not (the table of refusals above): the
// bounds are where they are said to be.
func TestTheShortestAndTheLongestRetentionAreAccepted(t *testing.T) {
	t.Parallel()

	for name, retention := range map[string]time.Duration{"the shortest": MinRetention, "the longest": MaxRetention, "ninety days": 90 * 24 * time.Hour} {
		u := sample("org-a", "2026-100001", "a")
		u.Retention = retention
		if err := u.validate(); err != nil {
			t.Errorf("%s (%s): %v", name, retention, err)
		}
	}
	if MinRetention != time.Second {
		t.Errorf("the shortest retention is %s, want a second: the database keeps an interval to the microsecond", MinRetention)
	}
}
