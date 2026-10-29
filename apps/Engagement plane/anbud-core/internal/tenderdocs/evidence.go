package tenderdocs

import (
	"context"
	"fmt"

	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/match"
)

// knownClass reports whether the gate knows what to make of a finding's class.
func knownClass(c match.FindingClass) bool {
	_, ok := classRank[c]
	return ok
}

// Evidence returns what the organisation's live uploads say about each of the notices, in the form
// the gate reads. A notice with no upload is not in the answer. It asks the store for at most
// docstore.MaxEvidenceNotices notices at a time, each once.
//
// A finding the rules cannot judge is left out: a capability this version has never heard of would be
// judged with no lead time at all, which is the wrong answer to whether the company can get it.
func (s *Service) Evidence(ctx context.Context, org string, noticeIDs []string) (map[string]match.Evidence, error) {
	out := map[string]match.Evidence{}
	if org == "" {
		return out, nil
	}
	seen := make(map[string]bool, len(noticeIDs))
	ids := make([]string, 0, len(noticeIDs))
	for _, id := range noticeIDs {
		if !seen[id] {
			seen[id] = true
			ids = append(ids, id)
		}
	}
	for len(ids) > 0 {
		n := min(len(ids), docstore.MaxEvidenceNotices)
		sctx, cancel := s.bounded(ctx)
		got, err := s.store.EvidenceFor(sctx, org, ids[:n])
		cancel()
		if err != nil {
			return nil, fmt.Errorf("tenderdocs: read the evidence: %w", err)
		}
		for id, ev := range got {
			out[id] = matchEvidence(ev)
		}
		ids = ids[n:]
	}
	return out, nil
}

func matchEvidence(in docstore.Evidence) match.Evidence {
	ev := match.Evidence{Documents: in.Documents, Unread: in.Unread, FindingsDropped: in.FindingsDropped}
	for _, f := range in.Findings {
		capability, class := match.Capability(f.Capability), match.FindingClass(f.Class)
		if _, known := match.DefaultLeadTimes[capability]; !known || !knownClass(class) {
			continue
		}
		ev.Findings = append(ev.Findings, match.Finding{
			Capability: capability, Class: class, Document: f.Document, Page: f.Page, Paragraph: f.Paragraph, Snippet: f.Snippet,
		})
	}
	return ev
}
