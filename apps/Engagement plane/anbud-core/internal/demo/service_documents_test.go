package demo

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
)

// evidenceCall is one question put to the evidence source.
type evidenceCall struct {
	org string
	ids []string
}

// fakeEvidence is what an organisation's uploaded documents say, by notice, and a record of the
// questions put to it.
type fakeEvidence struct {
	byNotice map[string]match.Evidence
	err      error

	mu    sync.Mutex
	calls []evidenceCall
}

func (f *fakeEvidence) Evidence(_ context.Context, org string, ids []string) (map[string]match.Evidence, error) {
	f.mu.Lock()
	f.calls = append(f.calls, evidenceCall{org: org, ids: slices.Clone(ids)})
	f.mu.Unlock()
	if f.err != nil {
		return nil, f.err
	}
	out := map[string]match.Evidence{}
	for _, id := range ids {
		if ev, ok := f.byNotice[id]; ok {
			out[id] = ev
		}
	}
	return out, nil
}

func (f *fakeEvidence) asked() []evidenceCall {
	f.mu.Lock()
	defer f.mu.Unlock()
	return slices.Clone(f.calls)
}

const documentOrg = "org-a"

// documentCorpus is a notice whose criteria are in the tender documents, one that is not for this
// company at all, and a result notice.
func documentCorpus() []rawstore.Document {
	return []rawstore.Document{
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "Åpen drift", CPV: "72212000", NUTS: "NO081", Deadline: in(20), PointToDocuments: true})),
		doc("2026-100002", eformstest.AwardNotice(eformstest.Notice{Title: "Tildelt drift", CPV: "72212000"})),
		doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{Title: "Åpen support", CPV: "72212000", NUTS: "NO081", Deadline: in(30)})),
	}
}

// isoRequired is what a specification that asks for ISO 9001 says, found by keyword on page 3.
var isoRequired = match.Evidence{
	Documents: 2, Unread: 1,
	Findings: []match.Finding{
		{Capability: match.CapISO9001, Class: match.ClassRequirement, Document: "konkurransegrunnlag.pdf", Page: 3, Snippet: "Leverandøren skal ha ISO 9001 eller tilsvarende."},
		{Capability: match.CapStartBank, Class: match.ClassMention, Document: "vedlegg/info.docx", Paragraph: 4, Snippet: "StartBANK er omtalt her."},
	},
}

func serviceWithEvidence(t *testing.T, notices NoticeSource, ev EvidenceSource, logs *bytes.Buffer) *Service {
	t.Helper()
	o := Options{Notices: notices, Evidence: ev, Now: func() time.Time { return now }}
	if logs != nil {
		logger := zerolog.New(logs)
		o.Logger = &logger
	}
	s, err := New(o)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return s
}

func findCheck(checks []CheckView, rule string) (CheckView, bool) {
	for _, c := range checks {
		if c.Rule == rule {
			return c, true
		}
	}
	return CheckView{}, false
}

func TestMatchesJudgesATenderByTheDocumentsTheOrganisationUploaded(t *testing.T) {
	t.Parallel()

	ev := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": isoRequired}}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg

	got, err := s.Matches(context.Background(), req)
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	var withDocs, without *Result
	for i := range got.Results {
		switch got.Results[i].NoticeID {
		case "2026-100001":
			withDocs = &got.Results[i]
		case "2026-100003":
			without = &got.Results[i]
		}
	}
	if withDocs == nil || without == nil {
		t.Fatalf("results = %v", resultIDs(got.Results))
	}

	check, ok := findCheck(withDocs.Gate, match.RuleDocumentRequirement)
	if !ok || check.Outcome != "conditional" {
		t.Fatalf("no conditional document_requirement check: %+v", withDocs.Gate)
	}
	if check.Cite.Field != "document" || check.Cite.Document != "konkurransegrunnlag.pdf" || check.Cite.Page != 3 ||
		check.Cite.Snippet != "Leverandøren skal ha ISO 9001 eller tilsvarende." || check.Cite.Index != nil {
		t.Errorf("the check does not cite the passage: %+v", check.Cite)
	}
	if withDocs.Verdict != "conditional" {
		t.Errorf("verdict = %q, want conditional: a document asks for a certificate the company does not hold", withDocs.Verdict)
	}
	if withDocs.Documents == nil || *withDocs.Documents != (DocumentsView{Read: 2, Unread: 1, Findings: 2}) {
		t.Errorf("documents = %+v", withDocs.Documents)
	}
	// The pointer check now says the documents were read as far as the rules go.
	pointer, ok := findCheck(withDocs.Gate, match.RuleDocuments)
	if !ok || !strings.Contains(pointer.Detail, "2 uploaded documents were read") {
		t.Errorf("the documents check = %+v", pointer)
	}
	// A notice nobody uploaded documents for is judged as it was.
	if without.Documents != nil || without.Verdict != "pass" {
		t.Errorf("a notice with no uploads: documents %+v verdict %q", without.Documents, without.Verdict)
	}
	if got.Documents == nil || !got.Documents.Checked || got.Documents.Notices != 1 || got.Documents.Reason != "" {
		t.Errorf("documents status = %+v", got.Documents)
	}
}

func TestMatchesAsksForTheEvidenceOnceForEveryTenderAndNothingElse(t *testing.T) {
	t.Parallel()

	ev := &fakeEvidence{}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	if _, err := s.Matches(context.Background(), req); err != nil {
		t.Fatal(err)
	}
	calls := ev.asked()
	if len(calls) != 1 || calls[0].org != documentOrg || !slices.Equal(calls[0].ids, []string{"2026-100001", "2026-100003"}) {
		t.Errorf("evidence was asked for %+v, want one call for the two tenders, not the result notice", calls)
	}
}

func TestMatchesWithoutAnOrganisationReadsNoDocuments(t *testing.T) {
	t.Parallel()

	ev := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": isoRequired}}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	got, err := s.Matches(context.Background(), mustDecode(t, itProfile)) // no Org
	if err != nil {
		t.Fatal(err)
	}
	if len(ev.asked()) != 0 {
		t.Errorf("evidence was asked for with no organisation")
	}
	if got.Documents != nil {
		t.Errorf("documents status = %+v, want none", got.Documents)
	}
	for _, r := range got.Results {
		if r.Documents != nil {
			t.Errorf("%s has documents", r.NoticeID)
		}
		if _, ok := findCheck(r.Gate, match.RuleDocumentRequirement); ok {
			t.Errorf("%s: a document check without an organisation", r.NoticeID)
		}
	}
}

func TestMatchesWithoutADocumentServiceSaysNothingOfDocuments(t *testing.T) {
	t.Parallel()

	s := newService(t, &fakeNotices{docs: documentCorpus()}, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	got, err := s.Matches(context.Background(), req)
	if err != nil || got.Documents != nil {
		t.Errorf("Matches = documents %+v, %v; want none: the feature is not configured", got.Documents, err)
	}
}

// Documents are advice. A store that is down costs the advice and nothing else: the matching goes on
// without it and says that it did.
func TestMatchesGoesOnWhenTheEvidenceCannotBeRead(t *testing.T) {
	t.Parallel()

	var logs bytes.Buffer
	ev := &fakeEvidence{err: errors.New("connection to db-host-7 refused")}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, &logs)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	got, err := s.Matches(context.Background(), req)
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if got.Documents == nil || got.Documents.Checked || got.Documents.Reason != "unavailable" {
		t.Errorf("documents status = %+v, want not checked: unavailable", got.Documents)
	}
	if len(got.Results) != 2 {
		t.Errorf("results = %v", resultIDs(got.Results))
	}
	if !strings.Contains(logs.String(), "db-host-7") {
		t.Errorf("the cause is not in the log: %s", logs.String())
	}
	if strings.Contains(logs.String(), documentOrg) {
		t.Errorf("the log names the organisation: %s", logs.String())
	}
}

// What a document can do to a verdict is bounded: it can make a check conditional, never failed, so
// widening a search cannot be changed by it either.
func TestDocumentsNeverChangeWhatWideningWouldAdd(t *testing.T) {
	t.Parallel()

	plain := newService(t, &fakeNotices{docs: documentCorpus()}, nil)
	ev := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": isoRequired, "2026-100003": isoRequired}}
	withDocs := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)

	req := mustDecode(t, `{"profile":{"cpv":["03000000"]},"strictness":"strict"}`)
	a, err := plain.Matches(context.Background(), req)
	if err != nil {
		t.Fatal(err)
	}
	req.Org = documentOrg
	b, err := withDocs.Matches(context.Background(), req)
	if err != nil {
		t.Fatal(err)
	}
	if fmt.Sprint(a.GainedByWidening) != fmt.Sprint(b.GainedByWidening) || a.Funnel.Gate["fail"] != b.Funnel.Gate["fail"] {
		t.Errorf("widening %v vs %v, fails %d vs %d", a.GainedByWidening, b.GainedByWidening, a.Funnel.Gate["fail"], b.Funnel.Gate["fail"])
	}
}

func TestAuditShowsWhatWasFoundInTheUploadedDocuments(t *testing.T) {
	t.Parallel()

	ev := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": isoRequired}}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg

	audit, err := s.Audit(context.Background(), "2026-100001", req)
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	d := audit.Documents
	if d == nil || d.Read != 2 || d.Unread != 1 || d.FindingsTotal != 2 || len(d.Findings) != 2 {
		t.Fatalf("documents = %+v", d)
	}
	want := FindingView{Document: "konkurransegrunnlag.pdf", Capability: "iso9001", Class: "requirement", Page: 3, Snippet: "Leverandøren skal ha ISO 9001 eller tilsvarende."}
	if d.Findings[0] != want {
		t.Errorf("finding = %+v\nwant %+v", d.Findings[0], want)
	}
	// Every class is shown: the audit is for a person to check, and a mention is as much a part of
	// what was read as a requirement.
	if d.Findings[1].Class != "mention" || d.Findings[1].Paragraph != 4 {
		t.Errorf("second finding = %+v", d.Findings[1])
	}
	if _, ok := findCheck(audit.Result.Gate, match.RuleDocumentRequirement); !ok {
		t.Errorf("the audit's gate has no document check")
	}
	if calls := ev.asked(); len(calls) != 1 || !slices.Equal(calls[0].ids, []string{"2026-100001"}) {
		t.Errorf("evidence was asked for %+v", calls)
	}
}

func TestAuditWithoutAnOrganisationOrWithoutUploadsHasNoDocuments(t *testing.T) {
	t.Parallel()

	ev := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": isoRequired}}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)

	audit, err := s.Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil || audit.Documents != nil {
		t.Errorf("without an organisation: %+v, %v", audit.Documents, err)
	}
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	audit, err = s.Audit(context.Background(), "2026-100003", req) // nobody uploaded for this one
	if err != nil || audit.Documents != nil {
		t.Errorf("a notice with no uploads: %+v, %v", audit.Documents, err)
	}
}

func TestAuditCapsTheFindingsItShows(t *testing.T) {
	t.Parallel()

	many := match.Evidence{Documents: 1}
	for i := 0; i < 250; i++ {
		many.Findings = append(many.Findings, match.Finding{Capability: match.CapISO9001, Class: match.ClassMention, Document: "a.pdf", Page: i + 1, Snippet: "ISO 9001"})
	}
	ev := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": many}}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	audit, err := s.Audit(context.Background(), "2026-100001", req)
	if err != nil {
		t.Fatal(err)
	}
	if d := audit.Documents; d == nil || len(d.Findings) != maxListed || d.FindingsTotal != 250 {
		t.Errorf("documents = %d shown of %d, want %d of 250", len(audit.Documents.Findings), audit.Documents.FindingsTotal, maxListed)
	}
}

func TestAuditGoesOnWhenTheEvidenceCannotBeRead(t *testing.T) {
	t.Parallel()

	ev := &fakeEvidence{err: errors.New("down")}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	audit, err := s.Audit(context.Background(), "2026-100001", req)
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if audit.Documents != nil || audit.DocumentsStatus == nil || audit.DocumentsStatus.Checked || audit.DocumentsStatus.Reason != "unavailable" {
		t.Errorf("documents %+v status %+v", audit.Documents, audit.DocumentsStatus)
	}
}

// A store that does not answer is given its deadline and no more: the matching goes on without the
// documents, and the scan slot is given back.
func TestMatchesGoesOnWhenTheEvidenceStoreDoesNotAnswerInTime(t *testing.T) {
	t.Parallel()

	blocked := &blockingEvidence{}
	s, err := New(Options{Notices: &fakeNotices{docs: documentCorpus()}, Evidence: blocked, DocumentsTimeout: 40 * time.Millisecond, MaxConcurrent: 1, Now: func() time.Time { return now }})
	if err != nil {
		t.Fatal(err)
	}
	req := mustDecode(t, itProfile)
	req.Org = documentOrg

	done := make(chan struct{})
	var got Matches
	go func() {
		defer close(done)
		var err error
		if got, err = s.Matches(context.Background(), req); err != nil {
			t.Errorf("Matches: %v", err)
		}
	}()
	select {
	case <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("Matches waited for a store that does not answer")
	}
	if got.Documents == nil || got.Documents.Checked || got.Documents.Reason != "unavailable" || len(got.Results) != 2 {
		t.Errorf("documents %+v, %d results", got.Documents, len(got.Results))
	}
	// The one slot is free again.
	if _, err := s.Matches(context.Background(), mustDecode(t, itProfile)); err != nil {
		t.Errorf("a second scan: %v", err)
	}
}

type blockingEvidence struct{}

func (blockingEvidence) Evidence(ctx context.Context, _ string, _ []string) (map[string]match.Evidence, error) {
	<-ctx.Done()
	return nil, ctx.Err()
}

// deadlineEvidence says how long the call to it had left.
type deadlineEvidence struct {
	mu   sync.Mutex
	left []time.Duration
	has  []bool
}

func (d *deadlineEvidence) Evidence(ctx context.Context, _ string, _ []string) (map[string]match.Evidence, error) {
	deadline, ok := ctx.Deadline()
	d.mu.Lock()
	defer d.mu.Unlock()
	d.has = append(d.has, ok)
	d.left = append(d.left, time.Until(deadline))
	return nil, nil
}

// Without a setting, the call for the documents has ten seconds: not none, and not the scan's twenty.
func TestTheCallForTheDocumentsHasTenSecondsWhenNoDeadlineIsSet(t *testing.T) {
	t.Parallel()

	ev := &deadlineEvidence{}
	s := serviceWithEvidence(t, &fakeNotices{docs: documentCorpus()}, ev, nil)
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	if _, err := s.Matches(context.Background(), req); err != nil {
		t.Fatal(err)
	}
	if len(ev.has) != 1 || !ev.has[0] {
		t.Fatalf("the evidence was asked %d times, with a deadline: %v", len(ev.has), ev.has)
	}
	if left := ev.left[0]; left < 8*time.Second || left > 10*time.Second {
		t.Errorf("the call had %s left, want about ten seconds", left)
	}
}

// A deadline that is set is the one the call has.
func TestTheCallForTheDocumentsHasTheDeadlineItWasGiven(t *testing.T) {
	t.Parallel()

	ev := &deadlineEvidence{}
	s, err := New(Options{Notices: &fakeNotices{docs: documentCorpus()}, Evidence: ev, DocumentsTimeout: 3 * time.Second, Now: func() time.Time { return now }})
	if err != nil {
		t.Fatal(err)
	}
	req := mustDecode(t, itProfile)
	req.Org = documentOrg
	if _, err := s.Matches(context.Background(), req); err != nil {
		t.Fatal(err)
	}
	if len(ev.has) != 1 || !ev.has[0] || ev.left[0] > 3*time.Second || ev.left[0] < time.Second {
		t.Errorf("deadline %v, time left %v: want about three seconds", ev.has, ev.left)
	}
}
