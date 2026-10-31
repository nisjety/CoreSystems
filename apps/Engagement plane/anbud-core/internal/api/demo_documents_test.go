package api

import (
	"context"
	"slices"
	"sync"
	"testing"

	"github.com/triodelab/anbud/internal/demo"
	"github.com/triodelab/anbud/internal/match"
)

// recordingEvidence says what a stored upload says for notice 2026-100001, and records the
// organisations it was asked for.
type recordingEvidence struct {
	mu   sync.Mutex
	orgs []string
	ids  [][]string
}

func (r *recordingEvidence) Evidence(_ context.Context, org string, ids []string) (map[string]match.Evidence, error) {
	r.mu.Lock()
	r.orgs = append(r.orgs, org)
	r.ids = append(r.ids, slices.Clone(ids))
	r.mu.Unlock()
	return map[string]match.Evidence{"2026-100001": {
		Documents: 1,
		Findings:  []match.Finding{{Capability: match.CapISO9001, Class: match.ClassRequirement, Document: "kg.pdf", Page: 2, Snippet: "Leverandøren skal ha ISO 9001."}},
	}}, nil
}

func (r *recordingEvidence) asked() []string {
	r.mu.Lock()
	defer r.mu.Unlock()
	return slices.Clone(r.orgs)
}

// The organisation a request is made for is the one the gateway authenticated, in X-Org-ID. It is not
// in the body, where a caller could name any tenant it liked.
func TestTheDemoReadsTheDocumentsOfTheOrganisationInTheHeader(t *testing.T) {
	t.Parallel()

	ev := &recordingEvidence{}
	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil, func(o *demo.Options) { o.Evidence = ev })

	for _, path := range []string{"/api/v1/demo/matches", "/api/v1/demo/matches/2026-100001"} {
		status, raw, _ := post(t, app, path, itProfile, map[string]string{"X-API-Key": serverKey, "X-Org-ID": "org-a"})
		if status != 200 {
			t.Fatalf("%s: %d %s", path, status, raw)
		}
		data, _ := decodeMap(t, raw)["data"].(map[string]any)
		docs, _ := data["documents"].(map[string]any)
		if path == "/api/v1/demo/matches" {
			if docs == nil || docs["checked"] != true || docs["notices"] != float64(1) {
				t.Errorf("%s: documents = %v", path, data["documents"])
			}
		} else if audit, _ := data["documents"].(map[string]any); audit == nil || audit["read"] != float64(1) {
			t.Errorf("%s: documents = %v", path, data["documents"])
		}
	}
	if got := ev.asked(); !slices.Equal(got, []string{"org-a", "org-a"}) {
		t.Errorf("evidence was asked for organisations %v", got)
	}
}

func TestAnOrganisationInTheBodyIsNotAnOrganisation(t *testing.T) {
	t.Parallel()

	ev := &recordingEvidence{}
	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil, func(o *demo.Options) { o.Evidence = ev })
	status, raw, _ := post(t, app, "/api/v1/demo/matches", `{"org":"org-b","profile":{"cpv":["72000000"]}}`, nil)
	if status != 400 {
		t.Errorf("a body that names an organisation: %d %s, want 400: the field is not known", status, raw)
	}
	if status, raw, _ := post(t, app, "/api/v1/demo/matches", itProfile, nil); status != 200 {
		t.Fatalf("without a header: %d %s", status, raw)
	}
	if got := ev.asked(); len(got) != 0 {
		t.Errorf("evidence was asked for %v without an organisation header", got)
	}
}
