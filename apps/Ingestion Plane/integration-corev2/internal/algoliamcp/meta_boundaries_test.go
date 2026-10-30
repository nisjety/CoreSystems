package algoliamcp

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/store"
	"io"
	"net/http"
	"strings"
	"testing"
)

func schemaTool(raw string) tool {
	var parsed schema
	_ = json.Unmarshal([]byte(raw), &parsed)
	return tool{InputSchema: parsed, RawSchema: json.RawMessage(raw)}
}
func TestArgumentSchemaProofRejectsUnsupportedMeaning(t *testing.T) {
	for _, tc := range []struct {
		schema string
		value  any
		valid  bool
	}{
		{`{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}`, "fixed", true},
		{`{"type":"object","$ref":"#/$defs/private","properties":{"value":{"type":"string"}}}`, "fixed", false},
		{`{"type":"object","oneOf":[],"properties":{"value":{"type":"string"}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","$ref":"#/$defs/private"}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","anyOf":[]}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","enum":[]}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","enum":null}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","enum":["fixed","fixed"]}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","enum":["fixed"]}}}`, "fixed", true},
		{`{"type":"object","properties":{"value":{"type":"string","minLength":10}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","maxLength":1}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","pattern":"^fixed$"}}}`, "fixed", true},
		{`{"type":"object","properties":{"value":{"type":"string","pattern":"^other$"}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","pattern":"["}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","pattern":null}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string","format":"date"}}}`, "2026-10-22", true},
		{`{"type":"object","properties":{"value":{"type":"string","format":"date"}}}`, "not-date", false},
		{`{"type":"object","properties":{"value":{"type":"string","format":"uri"}}}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"integer","minimum":0,"maximum":10}}}`, 8, true},
		{`{"type":"object","properties":{"value":{"type":"integer","minimum":10}}}`, 8, false},
		{`{"type":"object","properties":{"value":{"type":"integer","maximum":1}}}`, 8, false},
		{`{"type":"object","properties":{"value":{"type":"integer","minimum":null}}}`, 8, false},
		{`{"type":"object","properties":{"value":{"type":"object","additionalProperties":true}}}`, map[string]any{"applicationId": "APP1234567"}, true},
		{`{"type":"object","properties":{"value":{"type":"object","additionalProperties":false}}}`, map[string]any{"applicationId": "APP1234567"}, false},
		{`{"type":"object","properties":{"value":{"type":"object","properties":{}}}}`, map[string]any{}, true},
		{`{"type":"object","properties":{"value":{"type":"string"}},"required":["userIntent"]}`, "fixed", false},
		{`{"type":"object","properties":{"value":{"type":"string"}}}`, []string{"unproved"}, false},
	} {
		if got := verifiedArguments(schemaTool(tc.schema), map[string]any{"value": tc.value}); got != tc.valid {
			t.Fatalf("schema proof got%v want%v %s", got, tc.valid, tc.schema)
		}
	}
}
func TestMetadataParsersRejectAmbiguityAndPagination(t *testing.T) {
	op := listIndices
	good := `{"operationName":"` + op + `","effect":"read"}`
	for _, raw := range []string{`[` + good + `]`, `{"operations":[` + good + `]}`, `{"results":[` + good + `]}`} {
		if !discoveryRead(json.RawMessage(raw), op) {
			t.Fatal("recognized metadata envelope rejected")
		}
	}
	for _, raw := range []string{`[` + good + `,` + good + `]`, `{"tools":[` + good + `],"hasMore":true}`, `{"tools":[` + good + `],"nextCursor":"next"}`, `{"tools":[],"tools":[` + good + `]}`, `{"tools":[],"operations":[]}`, `[{"operationName":"delete_index","effect":"read"}]`, `[{"operationName":"` + op + `","name":"other","effect":"read"}]`} {
		if discoveryRead(json.RawMessage(raw), op) {
			t.Fatalf("ambiguous metadata accepted %s", raw)
		}
	}
	schema := `{"type":"object","properties":{"applicationId":{"type":"string","enum":["APP1234567"]}},"required":["applicationId"]}`
	for _, key := range []string{"inputSchema", "schema", "parameters"} {
		raw := `{"tool":{"name":"` + op + `","effect":"read","` + key + `":` + schema + `}}`
		if _, ok := describedRead(json.RawMessage(raw), op); !ok {
			t.Fatal("matching read description rejected")
		}
	}
	for _, raw := range []string{`{"operationName":"other","effect":"read","inputSchema":` + schema + `}`, `{"operationName":"` + op + `","effect":"write","inputSchema":` + schema + `}`, `{"operationName":"` + op + `","effect":"read","inputSchema":` + schema + `,"schema":` + schema + `}`, `{"operation":` + good + `,"tool":` + good + `}`} {
		if _, ok := describedRead(json.RawMessage(raw), op); ok {
			t.Fatal("unproved description accepted")
		}
	}
	for _, envelope := range []string{`{"result":{"count":0}}`, `{"data":{"count":0}}`} {
		if string(readPayload(json.RawMessage(envelope))) != `{"count":0}` {
			t.Fatal("recognized payload not unwrapped")
		}
	}
}
func TestRequiredPaginationAndRequestLocalDescriptionCache(t *testing.T) {
	f := &metaFixture{catalog: strings.Replace(metaCatalog, `"required":[]`, `"required":["query","limit","offset"]`, 1)}
	s := metaService(t, f)
	session, tools, done, err := s.open(context.Background(), "org-1", "conn_one")
	if err != nil {
		t.Fatal(err)
	}
	defer done()
	for i := 0; i < 2; i++ {
		if _, err := session.resolve(context.Background(), tools, listIndices); err != nil {
			t.Fatal(err)
		}
	}
	if len(f.calls) != 2 {
		t.Fatal("description not cached within request")
	}
	session.id = 14
	if _, err := session.rpc(context.Background(), "tools/list", nil); err == nil {
		t.Fatal("total RPC ceiling bypassed")
	}
}
func TestMetaRevocationBetweenDescribeAndReadAndDuringFinalRead(t *testing.T) {
	for _, phase := range []string{"describe", "execute"} {
		t.Run(phase, func(t *testing.T) {
			f := &metaFixture{}
			s := metaService(t, f)
			repo := s.repo.(*store.MemoryRepository)
			original := s.http.Transport
			s.http.Transport = fixtureTransport(func(r *http.Request) (*http.Response, error) {
				body, _ := io.ReadAll(r.Body)
				r.Body = io.NopCloser(bytes.NewReader(body))
				res, err := original.RoundTrip(r)
				text := string(body)
				revoke := phase == "describe" && strings.Contains(text, describeTool) || phase == "execute" && strings.Contains(text, callReadTool) && strings.Contains(text, noResults)
				if revoke {
					c, _ := repo.GetConnection(context.Background(), "conn_one")
					c.Status = "deleted"
					_, _ = repo.UpsertConnection(context.Background(), c)
				}
				return res, err
			})
			report, err := s.Report(context.Background(), "org-1", ReportInput{WebsiteID: "web_12345678901234567890123456789012", Site: "https://triodelab.no/", WebsiteRevision: 1, ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: "public_index"})
			if err == nil || report.Status == "connected" {
				t.Fatal("revoked grant produced report")
			}
			if phase == "describe" && f.readCalls != 0 {
				t.Fatal("read executed after description revocation")
			}
		})
	}
}
