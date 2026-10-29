package algoliamcp

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
)

const metaCatalog = `{"tools":[{"name":"algolia_discover_tools","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"},"offset":{"type":"integer"}},"required":[]}},{"name":"algolia_describe_tool","inputSchema":{"type":"object","properties":{"operationName":{"type":"string"},"originalQuery":{"type":"string"},"userIntent":{"type":"string"}},"required":["operationName"]}},{"name":"algolia_call_read_tool","inputSchema":{"type":"object","properties":{"operationName":{"type":"string"},"arguments":{"type":"object"}},"required":["operationName","arguments"]}}]}`

type metaFixture struct {
	catalog                string
	effect                 string
	describedEffect        string
	badEnvelope            bool
	calls                  []string
	readCalls              int
	listSchema             string
	indicesPayload         string
	probePayload           string
	queries                []string
	applicationListSchema  string
	applicationListPayload string
	termsSchema            string
	topPayload             string
	noResultPayload        string
	aggregatePayload       string
}

func metaService(t *testing.T, f *metaFixture) *Service {
	t.Helper()
	s, _, _ := fixtureService(t)
	if f.catalog == "" {
		f.catalog = metaCatalog
	}
	if f.effect == "" {
		f.effect = "read"
	}
	if f.describedEffect == "" {
		f.describedEffect = "read"
	}
	s.http = &http.Client{Transport: fixtureTransport(func(r *http.Request) (*http.Response, error) {
		var q struct {
			ID     int             `json:"id"`
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
		}
		_ = json.NewDecoder(r.Body).Decode(&q)
		var result any = map[string]any{}
		switch q.Method {
		case "initialize":
			result = map[string]any{"protocolVersion": "2025-03-26"}
		case "notifications/initialized":
			return &http.Response{StatusCode: 202, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(""))}, nil
		case "tools/list":
			result = json.RawMessage(f.catalog)
		case "tools/call":
			var p struct {
				Name string                     `json:"name"`
				Args map[string]json.RawMessage `json:"arguments"`
			}
			_ = json.Unmarshal(q.Params, &p)
			f.calls = append(f.calls, p.Name)
			for _, forbidden := range []string{"originalQuery", "userIntent"} {
				if _, exists := p.Args[forbidden]; exists {
					t.Fatal("prompt sent to metadata provider")
				}
			}
			var op string
			var payload any
			switch p.Name {
			case "algolia_discover_tools":
				_ = json.Unmarshal(p.Args["query"], &op)
				f.queries = append(f.queries, op)
				if op != listIndices && op != noResults && op != searchCount && op != applicationList && op != topSearches && op != searchesNoResults && op != "applications" {
					t.Fatal("nonfixed discovery query")
				}
				payload = map[string]any{"tools": []any{map[string]any{"operationName": op, "effect": f.effect}}}
				if op == "applications" {
					payload = json.RawMessage(f.probePayload)
				}
			case "algolia_describe_tool":
				_ = json.Unmarshal(p.Args["operationName"], &op)
				var schema any
				if op == listIndices {
					listSchema := f.listSchema
					if listSchema == "" {
						listSchema = `{"type":"object","properties":{"applicationId":{"type":"string","enum":["APP1234567"]}},"required":["applicationId"]}`
					}
					_ = json.Unmarshal([]byte(listSchema), &schema)
				} else if op == applicationList {
					input := f.applicationListSchema
					if input == "" {
						input = `{"type":"object","properties":{},"required":[]}`
					}
					_ = json.Unmarshal([]byte(input), &schema)
				} else if op == topSearches || op == searchesNoResults {
					input := f.termsSchema
					if input == "" {
						input = `{"type":"object","properties":{"applicationId":{"type":"string"},"indexName":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":1000}},"required":["applicationId","indexName","startDate","endDate"]}`
					}
					_ = json.Unmarshal([]byte(input), &schema)
				} else {
					_ = json.Unmarshal([]byte(`{"type":"object","properties":{"applicationId":{"type":"string"},"indexName":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":["applicationId","indexName","startDate","endDate"]}`), &schema)
				}
				payload = map[string]any{"operationName": op, "effect": f.describedEffect, "inputSchema": schema}
			case "algolia_call_read_tool":
				f.readCalls++
				_ = json.Unmarshal(p.Args["operationName"], &op)
				if op == listIndices {
					payload = json.RawMessage(`{"items":[{"name":"public_index"}]}`)
					if f.indicesPayload != "" {
						payload = json.RawMessage(f.indicesPayload)
					}
				} else if op == applicationList {
					if f.applicationListPayload == "" {
						payload = json.RawMessage(`{"applications":[{"applicationId":"APP1234567"}]}`)
					} else {
						payload = json.RawMessage(f.applicationListPayload)
					}
				} else if op == noResults {
					payload = json.RawMessage(`{"count":10,"noResultCount":2,"rate":0.2,"dates":[{"date":"2026-10-22","count":10,"noResultCount":2,"rate":0.2}]}`)
					if f.aggregatePayload != "" {
						payload = json.RawMessage(f.aggregatePayload)
					}
				} else if op == topSearches || op == searchesNoResults {
					var args map[string]json.RawMessage
					_ = json.Unmarshal(p.Args["arguments"], &args)
					if string(args["limit"]) != "20" || string(args["startDate"]) != `"2026-10-16"` || string(args["endDate"]) != `"2026-10-22"` {
						t.Fatal("unbounded or wrong-window terms read")
					}
					payload = json.RawMessage(`{"searches":[{"search":"fixture-term","count":2,"nbHits":3}]}`)
					if op == topSearches && f.topPayload != "" {
						payload = json.RawMessage(f.topPayload)
					}
					if op == searchesNoResults && f.noResultPayload != "" {
						payload = json.RawMessage(f.noResultPayload)
					}
				} else {
					t.Fatal("nonallowlisted operation executed")
				}
			default:
				t.Fatal("direct leaf or arbitrary metatool execution")
			}
			if f.badEnvelope && p.Name == "algolia_discover_tools" {
				payload = map[string]any{"private_message": "secret-token", "unknown_results": []any{}}
			}
			encoded, _ := json.Marshal(payload)
			result = map[string]any{"content": []any{map[string]any{"type": "text", "text": string(encoded)}}}
		default:
			t.Fatal("unexpected RPC")
		}
		encoded, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": q.ID, "result": result})
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(string(encoded)))}, nil
	})}
	return s
}
func TestMetaDiscoveryDescribeAndAggregateRead(t *testing.T) {
	f := &metaFixture{}
	s := metaService(t, f)
	apps, err := s.Applications(context.Background(), "org-1", "conn_one")
	if err != nil || len(apps) != 1 {
		t.Fatalf("apps %v %v", apps, err)
	}
	if f.readCalls != 1 || len(f.calls) != 3 {
		t.Fatal("application discovery fetched records or eagerly required analytics")
	}
	report, err := s.Report(context.Background(), "org-1", ReportInput{WebsiteID: "web_12345678901234567890123456789012", Site: "https://triodelab.no/", WebsiteRevision: 1, ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: "public_index"})
	if err != nil || report.Status != "connected" || *report.Algolia.Analytics.Searches != 10 {
		t.Fatalf("meta report %+v %v", report, err)
	}
	if f.readCalls != 5 {
		t.Fatal("unexpected provider reads")
	}
}
func TestMetaFlowFailsClosedWithoutReadProofOrKnownOuterSchema(t *testing.T) {
	for _, f := range []*metaFixture{{effect: "write"}, {describedEffect: "write"}, {badEnvelope: true}, {catalog: strings.Replace(metaCatalog, `"operationName"]}`, `"operationName","originalQuery"]}`, 1)}} {
		s := metaService(t, f)
		var events []Diagnostic
		s.diagnostic = func(d Diagnostic) { events = append(events, d) }
		if _, err := s.Applications(context.Background(), "org-1", "conn_one"); err == nil {
			t.Fatal("unverified meta discovery accepted")
		}
		if f.readCalls != 0 {
			t.Fatal("unsafe read wrapper executed")
		}
		raw, _ := json.Marshal(events)
		if strings.Contains(string(raw), "secret-token") || strings.Contains(string(raw), "private_message") {
			t.Fatal("meta payload logged")
		}
	}
}
