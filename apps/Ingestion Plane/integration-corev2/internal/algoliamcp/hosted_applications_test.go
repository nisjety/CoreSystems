package algoliamcp

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/store"
)

func TestHostedApplicationsNormalizeLiveProvenEnvelopeOnly(t *testing.T) {
	for _, tc := range []struct {
		raw   string
		count int
		valid bool
	}{
		{`{"applications":[{"applicationId":"APP1234567","name":"private-name","apiKey":"private-key"}]}`, 1, true},
		{`{"applications":[]}`, 0, true}, {`{}`, 0, false}, {`{"applications":null}`, 0, false}, {`{"items":[]}`, 0, false},
		{`{"applications":[{"applicationId":"APP1234567"},{"applicationId":"APP1234567"}]}`, 0, false},
		{`{"applications":[{"applicationId":"https://private.invalid"}]}`, 0, false}, {`{"applications":[{"id":"APP1234567"}]}`, 0, false},
		{`{"applications":[],"nextCursor":"private"}`, 0, false}, {`{"applications":[],"error":"private"}`, 0, false}, {`{"applications":[],"data":[]}`, 0, false},
		{`{"applications":[{"applicationId":"APP1234567","id":"OTHER12345"}]}`, 0, false}, {`{"applications":[],"applications":[]}`, 0, false},
	} {
		items, err := parseHostedApplications(json.RawMessage(tc.raw))
		if (err == nil) != tc.valid || len(items) != tc.count {
			t.Fatalf("unproved app list %s %v", tc.raw, err)
		}
		encoded, _ := json.Marshal(items)
		if strings.Contains(string(encoded), "private-") || strings.Contains(string(encoded), "apiKey") {
			t.Fatal("raw metadata escaped")
		}
	}
	rows := make([]map[string]string, 129)
	for i := range rows {
		rows[i] = map[string]string{"applicationId": "APP1234567"}
	}
	raw, _ := json.Marshal(map[string]any{"applications": rows})
	if _, err := parseHostedApplications(raw); err == nil {
		t.Fatal("oversized list accepted")
	}
}

func TestHostedApplicationsUsesDirectFixedMetadataPath(t *testing.T) {
	f := &metaFixture{applicationListSchema: `{"type":"object","properties":{},"required":[]}`, applicationListPayload: `{"applications":[{"applicationId":"APP1234567"}]}`}
	s := metaService(t, f)
	items, err := s.Applications(context.Background(), "org-1", "conn_one")
	if err != nil || len(items) != 1 || items[0].ID != "APP1234567" || len(f.calls) != 3 || len(f.queries) != 1 || f.queries[0] != applicationList {
		t.Fatalf("hosted flow not direct %v %v %v", items, err, f.calls)
	}
}

func TestHostedApplicationReadDropsResultsAfterRevocation(t *testing.T) {
	for _, boundary := range []string{"before-read", "during-read", "wrong-org"} {
		t.Run(boundary, func(t *testing.T) {
			f := &metaFixture{}
			s := metaService(t, f)
			repo := s.repo.(*store.MemoryRepository)
			original := s.http.Transport
			s.http.Transport = fixtureTransport(func(r *http.Request) (*http.Response, error) {
				body, _ := io.ReadAll(r.Body)
				r.Body = io.NopCloser(bytes.NewReader(body))
				response, err := original.RoundTrip(r)
				revoke := boundary == "before-read" && strings.Contains(string(body), describeTool) || boundary == "during-read" && strings.Contains(string(body), callReadTool)
				if revoke {
					c, _ := repo.GetConnection(context.Background(), "conn_one")
					c.Status = "deleted"
					_, _ = repo.UpsertConnection(context.Background(), c)
				}
				return response, err
			})
			org := "org-1"
			if boundary == "wrong-org" {
				org = "other-org"
			}
			if items, err := s.Applications(context.Background(), org, "conn_one"); err == nil || items != nil {
				t.Fatal("revoked/cross-org app list returned")
			}
			if boundary != "during-read" && f.readCalls != 0 {
				t.Fatal("read after denial")
			}
		})
	}
}
