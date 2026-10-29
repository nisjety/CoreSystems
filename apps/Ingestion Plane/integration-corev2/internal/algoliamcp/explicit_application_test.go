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

const unenumeratedApplicationSchema = `{"type":"object","properties":{"applicationId":{"type":"string"}},"required":["applicationId"]}`

func TestExplicitApplicationUsesProviderListProofWithoutEnumerating(t *testing.T) {
	f := &metaFixture{listSchema: unenumeratedApplicationSchema}
	s := metaService(t, f)
	indices, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567")
	if err != nil || len(indices) != 1 || indices[0].Name != "public_index" || f.readCalls != 1 {
		t.Fatalf("explicit provider proof missing: %v %v reads=%d", indices, err, f.readCalls)
	}
	resource, err := s.Resources(context.Background(), "org-1", ResourceInput{ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: "absent_index"})
	if err != nil || resource.Status != "unavailable" {
		t.Fatalf("absent index allowed: %+v %v", resource, err)
	}
}

func TestExplicitApplicationStillEnforcesSchemasAndProviderEvidence(t *testing.T) {
	for _, property := range []string{`{"type":"string","enum":[]}`, `{"type":"string","enum":null}`, `{"type":"string","enum":["OTHER12345"]}`, `{"type":"string","enum":["APP1234567","APP1234567"]}`, `{"type":"string","pattern":"^OTHER"}`, `{"type":"string","$ref":"#/private"}`} {
		t.Run(property, func(t *testing.T) {
			f := &metaFixture{listSchema: strings.Replace(unenumeratedApplicationSchema, `{"type":"string"}`, property, 1)}
			s := metaService(t, f)
			if _, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567"); err == nil || f.readCalls != 0 {
				t.Fatal("unproved application schema dispatched")
			}
		})
	}
	for _, payload := range []string{`{}`, `{"items":null}`, `{"items":[{"name":"private?query"}]}`} {
		f := &metaFixture{listSchema: unenumeratedApplicationSchema, indicesPayload: payload}
		s := metaService(t, f)
		if _, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567"); err == nil {
			t.Fatal("untyped provider response proved access")
		}
	}
	f := &metaFixture{listSchema: unenumeratedApplicationSchema, indicesPayload: `{"items":[]}`}
	s := metaService(t, f)
	if items, err := s.Indices(context.Background(), "org-1", "conn_one", "APP1234567"); err != nil || items == nil || len(items) != 0 {
		t.Fatal("valid empty provider list was not preserved")
	}
}

func TestMissingApplicationEnumMakesOnlyBoundedMetadataProbe(t *testing.T) {
	f := &metaFixture{listSchema: unenumeratedApplicationSchema, probePayload: `{"tools":[{"name":"algolia_list_applications","effect":"read","description":"secret-token"},{"name":"APP1234567","effect":"read"},{"name":"algolia_delete_application","effect":"write"}]}`}
	s := metaService(t, f)
	var events []Diagnostic
	s.diagnostic = func(d Diagnostic) { events = append(events, d) }
	session, tools, done, err := s.open(context.Background(), "org-1", "conn_one")
	if err != nil {
		t.Fatal(err)
	}
	defer done()
	tools, err = session.resolve(context.Background(), tools, listIndices)
	if err != nil {
		t.Fatal(err)
	}
	shape := session.probeApplications(context.Background(), tools)
	s.applicationSchemaFailure(tools, session.catalog, &shape)
	if f.readCalls != 0 || len(f.queries) < 2 || f.queries[1] != "applications" {
		t.Fatal("missing one fixed metadata-only probe")
	}
	raw, _ := json.Marshal(events)
	if !strings.Contains(string(raw), "algolia_list_applications") || !strings.Contains(string(raw), `"read":false`) || strings.Contains(string(raw), "secret-token") || strings.Contains(string(raw), "APP1234567") {
		t.Fatalf("unexpected diagnostic projection: %s", raw)
	}
}

func TestExplicitApplicationCannotBypassTenantOrRevocation(t *testing.T) {
	for _, boundary := range []string{"wrong-org", "revoke-on-list", "401-on-list"} {
		t.Run(boundary, func(t *testing.T) {
			f := &metaFixture{listSchema: unenumeratedApplicationSchema}
			s := metaService(t, f)
			repo := s.repo.(*store.MemoryRepository)
			original := s.http.Transport
			s.http.Transport = fixtureTransport(func(r *http.Request) (*http.Response, error) {
				body, _ := io.ReadAll(r.Body)
				r.Body = io.NopCloser(bytes.NewReader(body))
				result, err := original.RoundTrip(r)
				if strings.Contains(string(body), callReadTool) {
					if boundary == "revoke-on-list" {
						c, _ := repo.GetConnection(context.Background(), "conn_one")
						c.Status = "deleted"
						_, _ = repo.UpsertConnection(context.Background(), c)
					}
					if boundary == "401-on-list" {
						_ = result.Body.Close()
						return &http.Response{StatusCode: 401, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(`{"private":"denied"}`))}, nil
					}
				}
				return result, err
			})
			org := "org-1"
			if boundary == "wrong-org" {
				org = "other-org"
			}
			if items, err := s.Indices(context.Background(), org, "conn_one", "APP1234567"); err == nil || items != nil {
				t.Fatal("unapproved list became authorization proof")
			}
			if boundary == "wrong-org" && len(f.calls) != 0 {
				t.Fatal("cross-org provider access")
			}
		})
	}
}

func TestApplicationProbeProjectionIsBoundedAndNeverExecutable(t *testing.T) {
	raw := json.RawMessage(`{"operations":[{"name":"one_read","effect":"read","description":"secret-token"},{"name":"one_read","effect":"write"},{"name":"two_read","operationName":"different_read","effect":"read"},{"name":"three_read","effect":"read"},{"name":"four_read","effect":"read"},{"name":"five_read","effect":"read"},{"name":"APP1234567","effect":"read"},{"name":"https://private.invalid","effect":"read"},{"name":"eyJhbGci.eyJsub.signature","effect":"read"}]}`)
	shape := applicationProbeShape(raw)
	if shape.Status != "observed" || shape.Entries != 9 || len(shape.Operations) != 3 {
		t.Fatalf("unbounded shape %+v", shape)
	}
	for _, op := range shape.Operations {
		if allowedLeaf(op.Name) {
			t.Fatal("probe modified call authority")
		}
	}
	encoded, _ := json.Marshal(shape)
	for _, forbidden := range []string{"secret-token", "APP1234567", "private.invalid", "eyJhbGci", "different_read", "two_read"} {
		if strings.Contains(string(encoded), forbidden) {
			t.Fatal("untrusted metadata leaked")
		}
	}
	for _, payload := range []string{`{"tools":[],"nextCursor":"private"}`, `{"tools":[],"hasMore":true}`, `{"tools":[],"tools":[]}`, `{"unknown":[]}`} {
		if got := applicationProbeShape(json.RawMessage(payload)); got.Status != "unsupported" || len(got.Operations) != 0 {
			t.Fatal("unsupported probe projected")
		}
	}
	if got := applicationProbeShape(json.RawMessage(`{"tools":[]}`)); got.Status != "observed" || got.Entries != 0 {
		t.Fatal("explicit empty metadata not preserved")
	}
}

func TestApplicationProbeUsesOnlyVerifiedFixedArguments(t *testing.T) {
	for _, catalog := range []string{strings.Replace(metaCatalog, `"query":{"type":"string"}`, `"query":{"type":"string","pattern":"^algolia_"}`, 1), strings.Replace(metaCatalog, `"required":[]`, `"required":["userIntent"]`, 1)} {
		f := &metaFixture{catalog: catalog}
		s := metaService(t, f)
		session, tools, done, err := s.open(context.Background(), "org-1", "conn_one")
		if err != nil {
			t.Fatal(err)
		}
		shape := session.probeApplications(context.Background(), tools)
		done()
		if shape.Status != "unsupported" || len(f.calls) != 0 {
			t.Fatal("unverified fixed query dispatched")
		}
	}
	f := &metaFixture{probePayload: `not-json`}
	s := metaService(t, f)
	session, tools, done, err := s.open(context.Background(), "org-1", "conn_one")
	if err != nil {
		t.Fatal(err)
	}
	defer done()
	if shape := session.probeApplications(context.Background(), tools); shape.Status != "unavailable" || f.readCalls != 0 {
		t.Fatal("invalid probe result accepted")
	}
}
