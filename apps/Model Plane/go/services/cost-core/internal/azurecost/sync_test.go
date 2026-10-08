package azurecost

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestDecodeRowsUsesNamedColumns(t *testing.T) {
	client := &Client{scope: "/subscriptions/sub-123"}
	body := `{"properties":{"columns":[{"name":"Currency"},{"name":"UsageDate"},{"name":"Meter"},{"name":"project"},{"name":"PreTaxCost"}],"rows":[["NOK",20260923,"GPT-4o input","foundry-prod",12.5]]}}`
	rows, _, err := decodeRows(client.scope, []byte(body))
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 1 {
		t.Fatalf("got %d rows", len(rows))
	}
	row := rows[0]
	if row.Scope != client.scope || row.Project != "foundry-prod" || row.Meter != "GPT-4o input" || row.Currency != "NOK" || row.Amount != 12.5 {
		t.Fatalf("unexpected decoded cost row: %#v", row)
	}
	if got := row.Date.Format("2006-01-02"); got != "2026-09-23" {
		t.Fatalf("usage date = %s", got)
	}
}

func TestDecodeRowsReadsFoundryProjectFromTagPair(t *testing.T) {
	body := `{"properties":{"columns":[{"name":"PreTaxCost"},{"name":"UsageDate"},{"name":"TagKey"},{"name":"TagValue"},{"name":"Meter"},{"name":"Currency"}],"rows":[[12.5,20260923,"project","foundry-prod","GPT-4o input","NOK"],[3.0,20260923,"department","foundry-prod","GPT-4o output","NOK"]]}}`
	rows, _, err := decodeRows("/subscriptions/sub-123/resourceGroups/rg-a", []byte(body))
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 2 || rows[0].Project != "foundry-prod" || rows[1].Project != untaggedProject {
		t.Fatalf("unexpected project attribution from tags: %#v", rows)
	}
}

func TestDecodeRowsPreservesRowsWithoutAMeterName(t *testing.T) {
	body := `{"properties":{"columns":[{"name":"PreTaxCost"},{"name":"UsageDate"},{"name":"TagKey"},{"name":"TagValue"},{"name":"Meter"},{"name":"Currency"}],"rows":[[12.5,20260923,"project","foundry-prod",null,"NOK"]]}}`
	rows, _, err := decodeRows("/subscriptions/sub-123/resourceGroups/rg-a", []byte(body))
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 1 || rows[0].Meter != unspecifiedMeter {
		t.Fatalf("unexpected row for missing meter: %#v", rows)
	}
}

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) { return f(request) }

func TestQueryFollowsAzurePagination(t *testing.T) {
	calls := 0
	client := &Client{
		tenantID: "tenant", clientID: "client", clientSecret: "secret", scope: "/subscriptions/sub-123",
		now: func() time.Time { return time.Date(2026, 10, 3, 0, 0, 0, 0, time.UTC) },
	}
	client.http = &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
		if strings.Contains(request.URL.Host, "login.microsoftonline.com") {
			return response(http.StatusOK, `{"access_token":"test-token"}`), nil
		}
		requestBody, err := io.ReadAll(request.Body)
		if err != nil || !strings.Contains(string(requestBody), `"type":"ActualCost"`) {
			t.Fatalf("pagination request did not contain the query body: %s (err=%v)", requestBody, err)
		}
		calls++
		if request.Method != http.MethodPost || request.Header.Get("Authorization") != "Bearer test-token" {
			t.Fatalf("unexpected query request: %s %s auth=%q", request.Method, request.URL, request.Header.Get("Authorization"))
		}
		if calls == 1 {
			return response(http.StatusOK, `{"properties":{"columns":[{"name":"Currency"},{"name":"UsageDate"},{"name":"Meter"},{"name":"project"},{"name":"PreTaxCost"}],"rows":[["USD",20260922,"m1","p1",1.5]],"nextLink":"https://management.azure.com/subscriptions/sub-123/query?api-version=2026-06-01&$skiptoken=abc"}}`), nil
		}
		return response(http.StatusOK, `{"properties":{"columns":[{"name":"Currency"},{"name":"UsageDate"},{"name":"Meter"},{"name":"project"},{"name":"PreTaxCost"}],"rows":[["USD",20260923,"m1","p1",2.5]]}}`), nil
	})}
	from := time.Date(2026, 7, 5, 0, 0, 0, 0, time.UTC)
	to := time.Date(2026, 10, 4, 0, 0, 0, 0, time.UTC)
	rows, err := client.query(context.Background(), from, to)
	if err != nil {
		t.Fatal(err)
	}
	if calls != 2 || len(rows) != 2 || rows[0].Amount != 1.5 || rows[1].Amount != 2.5 {
		t.Fatalf("pagination returned calls=%d rows=%#v", calls, rows)
	}
}

func TestQueryRetriesCostManagementThrottle(t *testing.T) {
	queryCalls := 0
	client := &Client{
		tenantID: "tenant", clientID: "client", clientSecret: "secret", scope: "/subscriptions/sub-123",
		http: &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
			if strings.Contains(request.URL.Host, "login.microsoftonline.com") {
				return response(http.StatusOK, `{"access_token":"test-token"}`), nil
			}
			queryCalls++
			if queryCalls == 1 {
				throttled := response(http.StatusTooManyRequests, `{}`)
				throttled.Header.Set("x-ms-ratelimit-microsoft.consumption-retry-after", "0")
				return throttled, nil
			}
			return response(http.StatusOK, `{"properties":{"columns":[{"name":"Currency"},{"name":"UsageDate"},{"name":"Meter"},{"name":"project"},{"name":"PreTaxCost"}],"rows":[]}}`), nil
		})},
	}
	from := time.Date(2026, 7, 5, 0, 0, 0, 0, time.UTC)
	to := time.Date(2026, 10, 4, 0, 0, 0, 0, time.UTC)
	if _, err := client.query(context.Background(), from, to); err != nil {
		t.Fatal(err)
	}
	if queryCalls != 2 {
		t.Fatalf("query calls = %d, want initial request plus one retry", queryCalls)
	}
}

func TestSyncRetryDelayGrowsToBoundedMaximum(t *testing.T) {
	var delay time.Duration
	for _, expected := range []time.Duration{time.Minute, 2 * time.Minute, 4 * time.Minute, 8 * time.Minute, 16 * time.Minute, maxSyncRetry, maxSyncRetry} {
		delay = nextSyncRetryDelay(delay)
		if delay != expected {
			t.Fatalf("retry delay = %s, want %s", delay, expected)
		}
	}
}

func response(status int, body string) *http.Response {
	return &http.Response{StatusCode: status, Body: io.NopCloser(strings.NewReader(body)), Header: make(http.Header)}
}

func TestDecodeRowsRejectsMissingBillingDimensions(t *testing.T) {
	client := &Client{scope: "/subscriptions/sub-123"}
	_, _, err := decodeRows(client.scope, []byte(`{"properties":{"columns":[{"name":"Currency"},{"name":"PreTaxCost"}],"rows":[["NOK",1.25]]}}`))
	if err == nil || !strings.Contains(err.Error(), "lacks required") {
		t.Fatalf("expected missing dimensions error, got %v", err)
	}
}

func TestDecodeRowsAcceptsEmptyResultsWithoutDimensions(t *testing.T) {
	rows, nextLink, err := decodeRows("/subscriptions/sub-123/resourceGroups/empty", []byte(`{"properties":{"columns":[],"rows":[]}}`))
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 0 || nextLink != "" {
		t.Fatalf("empty Azure result = rows %#v, nextLink %q", rows, nextLink)
	}
}

func TestFromEnvRejectsPartialConfiguration(t *testing.T) {
	for _, key := range []string{"AZURE_COST_TENANT_ID", "AZURE_COST_CLIENT_ID", "AZURE_COST_CLIENT_SECRET", "AZURE_COST_SCOPE", "AZURE_COST_SCOPES_JSON", "AZURE_COST_PROJECT_ORG_MAP_JSON"} {
		t.Setenv(key, "")
	}
	t.Setenv("AZURE_COST_TENANT_ID", "tenant")
	if client, err := FromEnv(); err == nil || client != nil {
		t.Fatalf("partial Azure config must fail closed, client=%v err=%v", client, err)
	}
}

func TestFromEnvSupportsMultipleNarrowScopes(t *testing.T) {
	t.Setenv("AZURE_COST_TENANT_ID", "tenant")
	t.Setenv("AZURE_COST_CLIENT_ID", "client")
	t.Setenv("AZURE_COST_CLIENT_SECRET", "secret")
	t.Setenv("AZURE_COST_SCOPE", "")
	t.Setenv("AZURE_COST_SCOPES_JSON", `["/subscriptions/sub-1/resourceGroups/rg-a","/subscriptions/sub-1/resourceGroups/rg-b"]`)
	t.Setenv("AZURE_COST_PROJECT_ORG_MAP_JSON", `{"project-a":"org-a","project-b":"org-b"}`)

	client, err := FromEnv()
	if err != nil {
		t.Fatal(err)
	}
	if client == nil || len(client.scopes) != 2 || client.scopes[0] != "/subscriptions/sub-1/resourceGroups/rg-a" || client.scopes[1] != "/subscriptions/sub-1/resourceGroups/rg-b" {
		t.Fatalf("unexpected configured scopes: %#v", client)
	}
}

func TestFromEnvWithoutProjectMapKeepsCostsUnassigned(t *testing.T) {
	t.Setenv("AZURE_COST_TENANT_ID", "tenant")
	t.Setenv("AZURE_COST_CLIENT_ID", "client")
	t.Setenv("AZURE_COST_CLIENT_SECRET", "secret")
	t.Setenv("AZURE_COST_SCOPE", "/subscriptions/sub-1/resourceGroups/rg-a")
	t.Setenv("AZURE_COST_SCOPES_JSON", "")
	t.Setenv("AZURE_COST_PROJECT_ORG_MAP_JSON", "")

	client, err := FromEnv()
	if err != nil {
		t.Fatal(err)
	}
	if client == nil || len(client.projectOrgs) != 0 {
		t.Fatalf("unmapped billing must remain unassigned, client=%#v", client)
	}
}

func TestFromEnvRejectsDuplicateScopesAndBothScopeSettings(t *testing.T) {
	for _, key := range []string{"AZURE_COST_TENANT_ID", "AZURE_COST_CLIENT_ID", "AZURE_COST_CLIENT_SECRET", "AZURE_COST_PROJECT_ORG_MAP_JSON"} {
		t.Setenv(key, "")
	}
	t.Setenv("AZURE_COST_TENANT_ID", "tenant")
	t.Setenv("AZURE_COST_CLIENT_ID", "client")
	t.Setenv("AZURE_COST_CLIENT_SECRET", "secret")
	t.Setenv("AZURE_COST_PROJECT_ORG_MAP_JSON", `{"project":"org"}`)
	t.Setenv("AZURE_COST_SCOPE", "/subscriptions/sub-1")
	t.Setenv("AZURE_COST_SCOPES_JSON", `not-json`)
	if client, err := FromEnv(); err == nil || client != nil {
		t.Fatalf("both scope forms must fail closed, client=%v err=%v", client, err)
	}
	t.Setenv("AZURE_COST_SCOPE", "")
	t.Setenv("AZURE_COST_SCOPES_JSON", `["/subscriptions/sub-1","/subscriptions/sub-1/"]`)
	if client, err := FromEnv(); err == nil || client != nil {
		t.Fatalf("duplicate scopes must fail closed, client=%v err=%v", client, err)
	}
}

type scopeCaptureRepository struct{ scopes []string }

func (r *scopeCaptureRepository) ReplaceAzureActualCosts(_ context.Context, scope string, _, _ time.Time, _ map[string]string, _ []Row) error {
	r.scopes = append(r.scopes, scope)
	return nil
}

func TestSyncQueriesAndReplacesEachConfiguredScope(t *testing.T) {
	scopes := []string{"/subscriptions/sub-1/resourceGroups/rg-a", "/subscriptions/sub-1/resourceGroups/rg-b"}
	client := &Client{
		tenantID: "tenant", clientID: "client", clientSecret: "secret", scope: scopes[0], scopes: scopes,
		projectOrgs: map[string]string{"project-a": "org-a"},
		now:         func() time.Time { return time.Date(2026, 10, 4, 0, 0, 0, 0, time.UTC) },
	}
	queried := []string{}
	client.http = &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
		if strings.Contains(request.URL.Host, "login.microsoftonline.com") {
			return response(http.StatusOK, `{"access_token":"test-token"}`), nil
		}
		queried = append(queried, request.URL.Path)
		return response(http.StatusOK, `{"properties":{"columns":[{"name":"Currency"},{"name":"UsageDate"},{"name":"Meter"},{"name":"project"},{"name":"PreTaxCost"}],"rows":[]}}`), nil
	})}
	repo := &scopeCaptureRepository{}
	if err := client.Sync(context.Background(), repo); err != nil {
		t.Fatal(err)
	}
	if len(queried) != 2 || len(repo.scopes) != 2 {
		t.Fatalf("queried %d scopes and replaced %d, want 2 each", len(queried), len(repo.scopes))
	}
	for i, scope := range scopes {
		if !strings.HasPrefix(queried[i], scope+"/") || repo.scopes[i] != scope {
			t.Fatalf("scope %d query=%q replaced=%q, want %q", i, queried[i], repo.scopes[i], scope)
		}
	}
}

func TestParseUsageDateAcceptsAzureDateForms(t *testing.T) {
	for _, value := range []any{"20260923", "2026-09-23", "2026-09-23T00:00:00Z"} {
		got, err := parseUsageDate(value)
		if err != nil {
			t.Fatalf("%v: %v", value, err)
		}
		if !got.Equal(time.Date(2026, 9, 23, 0, 0, 0, 0, time.UTC)) {
			t.Fatalf("%v parsed as %v", value, got)
		}
	}
}
