package docstore

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// heavy is an upload of a notice with as many findings as an upload keeps, each different from those of every other upload.
func heavy(org, notice string, n int) NewUpload {
	u := sample(org, notice, fmt.Sprintf("%s %s heavy %d", org, notice, n))
	u.Findings = uploadOf(n)
	return u
}

// A notice with five uploads of as many findings as an upload keeps comes to five times as many distinct findings. The
// evidence holds what one upload may keep and says how many it left out, so that nothing is lost without a number, and
// the sum of the two is every finding there is.
func TestEvidenceForHoldsWhatANoticeKeepsAndCountsTheRest(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	for n := 0; n < 5; n++ {
		if _, created, err := s.Create(ctx, heavy("org-a", "2026-100001", n)); err != nil || !created {
			t.Fatalf("upload %d: created %v, %s", n, created, dbtest.Describe(err))
		}
	}
	got, err := s.EvidenceFor(ctx, "org-a", []string{"2026-100001"})
	if err != nil {
		t.Fatalf("EvidenceFor: %s", dbtest.Describe(err))
	}
	ev := got["2026-100001"]
	if len(ev.Findings) != MaxEvidencePerNotice || ev.FindingsDropped != 5*8*MaxEvidencePerCapability-MaxEvidencePerNotice {
		t.Errorf("%d findings held and %d left out; want %d and %d", len(ev.Findings), ev.FindingsDropped, MaxEvidencePerNotice, 5*8*MaxEvidencePerCapability-MaxEvidencePerNotice)
	}
	perCapability := map[string]int{}
	for _, f := range ev.Findings {
		perCapability[f.Capability]++
	}
	for capability, n := range perCapability {
		if n != MaxEvidencePerCapability {
			t.Errorf("%s: %d held, want %d", capability, n, MaxEvidencePerCapability)
		}
	}
	if ev.Documents != 1 || ev.Unread != 2 {
		t.Errorf("documents %d, unread %d; want 1 and 2: what is said of the documents is whole", ev.Documents, ev.Unread)
	}
}

// The budget is the call's, and the call is for all the notices it is asked about: of six notices that are each a heavy
// upload, the first use it up, and the others hold the first requirement of each capability, and each says how many
// it left out.
func TestEvidenceForSharesWhatItMayHoldOnToBetweenTheNotices(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	var notices []string
	for n := 0; n < 6; n++ {
		notice := fmt.Sprintf("2026-10000%d", n+1)
		notices = append(notices, notice)
		if _, _, err := s.Create(ctx, heavy("org-a", notice, n)); err != nil {
			t.Fatalf("notice %d: %s", n, dbtest.Describe(err))
		}
	}
	budget := 0
	for _, f := range uploadOf(0)[:120] {
		budget += findingBytes(f)
	}
	s.evidenceBytes = budget

	got, err := s.EvidenceFor(ctx, "org-a", notices)
	if err != nil {
		t.Fatalf("EvidenceFor: %s", dbtest.Describe(err))
	}
	held := 0
	for i, notice := range notices {
		ev := got[notice]
		if len(ev.Findings)+ev.FindingsDropped != 8*MaxEvidencePerCapability {
			t.Errorf("%s: %d kept and %d left out; want every one of the %d counted", notice, len(ev.Findings), ev.FindingsDropped, 8*MaxEvidencePerCapability)
		}
		if i > 0 && len(ev.Findings) != 8 {
			t.Errorf("%s holds %d findings, want the first requirement of each of the 8 capabilities and no more, for the budget is used up", notice, len(ev.Findings))
		}
		held += len(ev.Findings)
	}
	if first := got[notices[0]]; len(first.Findings) < 100 || len(first.Findings) > 140 {
		t.Errorf("the first notice holds %d findings of a budget that is room for about 120", len(first.Findings))
	}
	if held > 120+5*8+20 {
		t.Errorf("%d findings held in all, of a budget for about 120 and 8 for each notice after the first", held)
	}
}

// The uploads of a notice are read together, the oldest first, and the notices one after another: so that the evidence of a
// notice is of every upload of it, however the uploads of the notices were interleaved when they were made.
func TestEvidenceForSumsTheUploadsOfANoticeWhateverOrderTheyWereMadeIn(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	notices := []string{"2026-100001", "2026-100002", "2026-100001", "2026-100002"}
	for i, notice := range notices {
		u := sample("org-a", notice, fmt.Sprintf("upload %d", i))
		u.Findings = []Finding{{Document: fmt.Sprintf("d%d.pdf", i), Capability: "iso9001", Class: "requirement", Page: i + 1, Snippet: fmt.Sprintf("finding %d", i)}}
		if _, _, err := s.Create(ctx, u); err != nil {
			t.Fatalf("upload %d: %s", i, dbtest.Describe(err))
		}
	}
	got, err := s.EvidenceFor(ctx, "org-a", []string{"2026-100001", "2026-100002"})
	if err != nil {
		t.Fatalf("EvidenceFor: %s", dbtest.Describe(err))
	}
	for notice, want := range map[string][]string{"2026-100001": {"finding 0", "finding 2"}, "2026-100002": {"finding 1", "finding 3"}} {
		var snippets []string
		for _, f := range got[notice].Findings {
			snippets = append(snippets, f.Snippet)
		}
		if fmt.Sprint(snippets) != fmt.Sprint(want) {
			t.Errorf("%s: findings %v, want %v: both of its uploads, the oldest first", notice, snippets, want)
		}
	}
}

// An upload that has expired is gone to every read before the purge has removed it, the evidence included: what it said is
// not what the organisation's documents say now.
func TestEvidenceForLeavesOutWhatHasExpired(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	docs, _ := json.Marshal([]Document{{Path: "old.pdf", Kind: "pdf", Size: 1, SHA256: sha("old"), Status: StatusRead}})
	findings, _ := json.Marshal([]Finding{{Document: "old.pdf", Capability: "achilles", Class: "requirement", Snippet: "EXPIRED: Achilles kreves."}})
	for _, notice := range []string{"2026-100001", "2026-100002"} {
		_, err := pool.Exec(ctx, `
INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, created_at, expires_at)
VALUES ('org-a', $1, $2, 1, $3::jsonb, $4::jsonb, 1, 0, now() - interval '3 days', now() - interval '1 day')`, notice, sha("expired "+notice), string(docs), string(findings))
		if err != nil {
			t.Fatalf("insert an expired upload: %s", dbtest.Describe(err))
		}
	}
	if _, _, err := s.Create(ctx, sample("org-a", "2026-100001", "live")); err != nil {
		t.Fatalf("Create: %s", dbtest.Describe(err))
	}
	got, err := s.EvidenceFor(ctx, "org-a", []string{"2026-100001", "2026-100002"})
	if err != nil {
		t.Fatalf("EvidenceFor: %s", dbtest.Describe(err))
	}
	if _, ok := got["2026-100002"]; ok {
		t.Errorf("there is evidence for a notice whose only upload has expired: %+v", got["2026-100002"])
	}
	ev := got["2026-100001"]
	for _, f := range ev.Findings {
		if strings.Contains(f.Snippet, "EXPIRED") {
			t.Errorf("the evidence holds a finding of an upload that has expired: %+v", f)
		}
	}
	if ev.Documents != 1 || ev.Unread != 2 {
		t.Errorf("documents %d, unread %d; want 1 and 2, those of the live upload alone", ev.Documents, ev.Unread)
	}
}

// What another organisation uploaded for the same notice is not what this one's documents say. The evidence names the
// organisation in its own statement and does not leave that to row level security, which the owner of the table is not
// held to: this is the statement tried with the policy out of the way, with an upload of the other that is different.
func TestEvidenceForNamesTheOrganisationWithoutRowLevelSecurity(t *testing.T) {
	s, _ := openStore(t) // the server's own role, which owns the tables and so bypasses the policy
	ctx := testCtx(t)
	mine := sample("org-a", "2026-100001", "a")
	theirs := sample("org-b", "2026-100001", "b")
	theirs.Documents = []Document{{Path: "hemmelig.pdf", Kind: "pdf", Size: 9, SHA256: sha("hemmelig"), Status: StatusRead}}
	theirs.Findings = []Finding{{Document: "hemmelig.pdf", Capability: "achilles", Class: "requirement", Page: 1, Snippet: "ORG-B: Achilles kreves."}}
	for _, u := range []NewUpload{mine, theirs} {
		if _, _, err := s.Create(ctx, u); err != nil {
			t.Fatalf("Create: %s", dbtest.Describe(err))
		}
	}
	got, err := s.EvidenceFor(ctx, "org-a", []string{"2026-100001"})
	if err != nil {
		t.Fatalf("EvidenceFor: %s", dbtest.Describe(err))
	}
	ev := got["2026-100001"]
	if ev.Documents != 1 || ev.Unread != 2 || len(ev.Findings) != 1 || strings.Contains(ev.Findings[0].Snippet, "ORG-B") {
		t.Errorf("org-a's evidence = %+v; want its own documents (1 read, 2 unread) and its own single finding, and nothing of org-b's", ev)
	}
	other, err := s.EvidenceFor(ctx, "org-b", []string{"2026-100001"})
	if err != nil || other["2026-100001"].Documents != 1 || len(other["2026-100001"].Findings) != 1 || !strings.Contains(other["2026-100001"].Findings[0].Snippet, "ORG-B") {
		t.Errorf("org-b's evidence = %+v, %v; want its own alone", other["2026-100001"], err)
	}
}
