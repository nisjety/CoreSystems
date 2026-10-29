package algoliamcp

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/store"
)

func termsReportInput() ReportInput {
	return ReportInput{WebsiteID: "web_12345678901234567890123456789012", Site: "https://triodelab.no/", WebsiteRevision: 1, ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: "public_index"}
}

func TestTermsAndAggregateComponentsAreIndependent(t *testing.T) {
	for _, tc := range []struct {
		aggregate, top, noResult                   string
		connected                                  bool
		topStatus, noResultStatus, aggregateStatus string
	}{
		{"", "", "", true, "connected", "connected", "connected"},
		{`{}`, "", "", true, "connected", "connected", "unavailable"},
		{"", `{}`, `{}`, true, "unavailable", "unavailable", "connected"},
		{`{}`, `{"searches":[]}`, `{}`, true, "connected", "unavailable", "unavailable"},
		{`{}`, `{}`, `{}`, false, "unavailable", "unavailable", "unavailable"},
	} {
		f := &metaFixture{aggregatePayload: tc.aggregate, topPayload: tc.top, noResultPayload: tc.noResult}
		s := metaService(t, f)
		var events []Diagnostic
		s.diagnostic = func(d Diagnostic) { events = append(events, d) }
		report, err := s.Report(context.Background(), "org-1", termsReportInput())
		if err != nil || (report.Status == "connected") != tc.connected || report.Algolia.TopSearches == nil || report.Algolia.NoResultTerms == nil {
			t.Fatalf("partial report %+v %v", report, err)
		}
		if report.Algolia.TopSearches.Status != tc.topStatus || report.Algolia.NoResultTerms.Status != tc.noResultStatus || report.Algolia.Analytics.Status != tc.aggregateStatus {
			t.Fatalf("component failure erased independent receipt %+v", report.Algolia)
		}
		encoded, _ := json.Marshal(events)
		if strings.Contains(string(encoded), "fixture-term") {
			t.Fatal("search terms logged")
		}
	}
}

func TestTermsArgumentsRequireActualTypedBoundedSchema(t *testing.T) {
	window := sitesources.Window{StartDate: "2026-10-16", EndDate: "2026-10-22"}
	base := `{"type":"object","properties":{"applicationId":{"type":"string"},"index":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"},"limit":{"type":"integer"}},"required":["applicationId","index","startDate","endDate","limit"]}`
	args, err := searchTermsArgs(schemaTool(base), "APP1234567", "public_index", window)
	if err != nil || args["limit"] != 20 || args["index"] != "public_index" {
		t.Fatal("actual aliases/limit not proved")
	}
	for _, schema := range []string{strings.Replace(base, `"type":"integer"`, `"type":"string"`, 1), strings.Replace(base, `"limit"]}`, `"limit","userIntent"]}`, 1), strings.Replace(base, `"limit":{"type":"integer"}`, `"limit":{"type":"integer","maximum":10}`, 1)} {
		if _, err := searchTermsArgs(schemaTool(schema), "APP1234567", "public_index", window); err == nil {
			t.Fatal("unverified terms args accepted")
		}
	}
}

func TestTermsSortDescendingBySearchCountWhenSchemaSupportsIt(t *testing.T) {
	base := `{"type":"object","properties":{"applicationId":{"type":"string"},"indexName":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"},"direction":{"type":"string","enum":["asc","desc"]},"orderBy":{"type":"string","enum":["searchCount"]}},"required":["applicationId","indexName","startDate","endDate","direction","orderBy"]}`
	args, err := searchTermsArgs(schemaTool(base), "APP1234567", "public_index", sitesources.Window{StartDate: "2026-10-16", EndDate: "2026-10-22"})
	if err != nil || args["direction"] != "desc" || args["orderBy"] != "searchCount" {
		t.Fatal("top terms used unproved default ordering")
	}
}

func TestTermsRevokeDuringFinalComponentDiscardsEarlierComponents(t *testing.T) {
	f := &metaFixture{}
	s := metaService(t, f)
	repo := s.repo.(*store.MemoryRepository)
	original := s.http.Transport
	s.http.Transport = fixtureTransport(func(r *http.Request) (*http.Response, error) {
		body, _ := io.ReadAll(r.Body)
		r.Body = io.NopCloser(bytes.NewReader(body))
		res, err := original.RoundTrip(r)
		if strings.Contains(string(body), callReadTool) && strings.Contains(string(body), searchesNoResults) {
			c, _ := repo.GetConnection(context.Background(), "conn_one")
			c.Status = "deleted"
			_, _ = repo.UpsertConnection(context.Background(), c)
		}
		return res, err
	})
	report, err := s.Report(context.Background(), "org-1", termsReportInput())
	if err == nil || report.Status == "connected" || report.Algolia.TopSearches.Rows != nil || report.Algolia.Analytics.Searches != nil {
		t.Fatal("revocation returned earlier private components")
	}
}

func TestReportBudgetRemainsBoundedAndSeparateFromMetadata(t *testing.T) {
	f := &metaFixture{}
	s := metaService(t, f)
	for _, tc := range []struct {
		budget time.Duration
		limit  int
	}{{20 * time.Second, 14}, {30 * time.Second, 18}} {
		session, _, done, err := s.openBudget(context.Background(), "org-1", "conn_one", tc.budget, tc.limit)
		if err != nil {
			t.Fatal(err)
		}
		deadline, ok := session.ctx.Deadline()
		if !ok || time.Until(deadline) > tc.budget || time.Until(deadline) < tc.budget-time.Second {
			t.Fatal("session deadline missing")
		}
		session.id = tc.limit
		if _, err := session.rpc(context.Background(), "tools/list", nil); err == nil {
			t.Fatal("RPC ceiling bypassed")
		}
		done()
	}
}
