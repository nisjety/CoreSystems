// Package azurecost imports Azure Cost Management's daily ActualCost rows.
// Those rows are organization/project financial totals; they are never
// attributed to individual users by Azure.
package azurecost

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"math"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

const (
	managementHost   = "https://management.azure.com"
	apiVersion       = "2026-06-01"
	maxResponse      = 8 << 20
	maxTotalResponse = 64 << 20
	maxPages         = 100
	maxQueryAttempts = 5
	maxRetryDelay    = 2 * time.Minute
	syncInterval     = 6 * time.Hour
	maxSyncRetry     = 30 * time.Minute
	untaggedProject  = "__untagged__"
	unspecifiedMeter = "Unspecified meter"
)

// Row is one source-reported actual-cost meter for a Foundry project and day.
type Row struct {
	Scope    string
	Project  string
	Meter    string
	Date     time.Time
	Amount   float64
	Currency string
}

// Repository stores source rows with the explicitly configured project → org
// mapping. Unmapped projects are rejected rather than assigned to a tenant.
type Repository interface {
	ReplaceAzureActualCosts(context.Context, string, time.Time, time.Time, map[string]string, []Row) error
}

type CurrencyTotal struct {
	Currency string  `json:"currency"`
	Amount   float64 `json:"amount"`
}

type Summary struct {
	Available       bool            `json:"available"`
	Rows            int64           `json:"rows"`
	ByCurrency      []CurrencyTotal `json:"by_currency"`
	LatestUsageDate *time.Time      `json:"latest_usage_date,omitempty"`
}

type Reader interface {
	AzureActualSummary(context.Context, string, *time.Time, *time.Time) (Summary, error)
}

type Client struct {
	tenantID     string
	clientID     string
	clientSecret string
	scope        string
	scopes       []string
	projectOrgs  map[string]string
	http         *http.Client
	now          func() time.Time
}

// FromEnv is disabled unless all auth and scope settings are present. Project
// mapping is optional; without it, imported rows remain unassigned. Use
// AZURE_COST_SCOPES_JSON to query multiple narrow ARM scopes with
// one principal; AZURE_COST_SCOPE remains supported for a single scope. The
// project map maps Azure Foundry project tag values to canonical Verevon orgs.
func FromEnv() (*Client, error) {
	values := []string{"AZURE_COST_TENANT_ID", "AZURE_COST_CLIENT_ID", "AZURE_COST_CLIENT_SECRET"}
	configured := 0
	for _, key := range values {
		if strings.TrimSpace(os.Getenv(key)) != "" {
			configured++
		}
	}
	singleScope := strings.TrimSpace(os.Getenv("AZURE_COST_SCOPE"))
	scopesJSON := strings.TrimSpace(os.Getenv("AZURE_COST_SCOPES_JSON"))
	if configured == 0 && singleScope == "" && scopesJSON == "" {
		return nil, nil
	}
	if configured != len(values) || (singleScope == "" && scopesJSON == "") {
		return nil, errors.New("Azure Cost Management requires tenant, client, secret, and one or more scopes")
	}
	if singleScope != "" && scopesJSON != "" {
		return nil, errors.New("configure either AZURE_COST_SCOPE or AZURE_COST_SCOPES_JSON, not both")
	}
	var mapping map[string]string
	projectMapping := strings.TrimSpace(os.Getenv("AZURE_COST_PROJECT_ORG_MAP_JSON"))
	if projectMapping == "" {
		mapping = map[string]string{}
	} else if err := json.Unmarshal([]byte(projectMapping), &mapping); err != nil || mapping == nil {
		return nil, errors.New("AZURE_COST_PROJECT_ORG_MAP_JSON must be a JSON object when provided")
	}
	canonical := make(map[string]string, len(mapping))
	for project, org := range mapping {
		project, org = strings.TrimSpace(project), strings.TrimSpace(org)
		if project == "" || org == "" || strings.ContainsAny(project+org, "\r\n\x00") {
			return nil, errors.New("Azure project-to-organization mappings must be non-empty canonical strings")
		}
		canonical[project] = org
	}
	scopes := []string{}
	if scopesJSON != "" {
		if err := json.Unmarshal([]byte(scopesJSON), &scopes); err != nil || len(scopes) == 0 {
			return nil, errors.New("AZURE_COST_SCOPES_JSON must be a non-empty JSON array of ARM scopes")
		}
	} else {
		scopes = append(scopes, singleScope)
	}
	seenScopes := make(map[string]struct{}, len(scopes))
	for index, scope := range scopes {
		scope = strings.TrimRight(strings.TrimSpace(scope), "/")
		if !validScope(scope) {
			return nil, errors.New("Azure Cost Management scopes must be valid ARM scopes under a subscription")
		}
		if _, exists := seenScopes[strings.ToLower(scope)]; exists {
			return nil, errors.New("Azure Cost Management scopes must be unique")
		}
		seenScopes[strings.ToLower(scope)] = struct{}{}
		scopes[index] = scope
	}
	return &Client{
		tenantID:     strings.TrimSpace(os.Getenv("AZURE_COST_TENANT_ID")),
		clientID:     strings.TrimSpace(os.Getenv("AZURE_COST_CLIENT_ID")),
		clientSecret: os.Getenv("AZURE_COST_CLIENT_SECRET"),
		scope:        scopes[0],
		scopes:       scopes,
		projectOrgs:  canonical,
		http:         &http.Client{Timeout: 25 * time.Second},
		now:          time.Now,
	}, nil
}

func validScope(scope string) bool {
	if strings.Contains(scope, "..") || strings.ContainsAny(scope, "?#\\") {
		return false
	}
	parts := strings.Split(strings.Trim(scope, "/"), "/")
	if len(parts) == 2 && strings.EqualFold(parts[0], "subscriptions") && parts[1] != "" {
		return true
	}
	return len(parts) == 4 && strings.EqualFold(parts[0], "subscriptions") && parts[1] != "" && strings.EqualFold(parts[2], "resourceGroups") && parts[3] != ""
}

// Run performs an initial sync and then refreshes daily actual-cost rows every
// six hours. Azure billing data can lag usage, so the API reports its latest
// source date rather than claiming real-time billing completeness.
func (c *Client) Run(ctx context.Context, repo Repository) {
	var retryDelay time.Duration
	for {
		nextDelay := syncInterval
		if err := c.Sync(ctx, repo); err != nil {
			if ctx.Err() == nil {
				// Never include request bodies, tokens, or the client secret in logs.
				fmt.Fprintf(os.Stderr, "[cost-core/azure-cost] daily billing sync failed: %v\n", err)
			}
			retryDelay = nextSyncRetryDelay(retryDelay)
			nextDelay = retryDelay
		} else {
			slog.Info("Azure Cost Management actual-cost sync completed")
			retryDelay = 0
		}
		timer := time.NewTimer(nextDelay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
	}
}

func nextSyncRetryDelay(previous time.Duration) time.Duration {
	if previous <= 0 {
		return time.Minute
	}
	return min(previous*2, maxSyncRetry)
}

// Sync queries a rolling 90-day period, allowing Azure's delayed/corrected rows
// to overwrite prior daily snapshots idempotently.
func (c *Client) Sync(ctx context.Context, repo Repository) error {
	if c == nil || repo == nil {
		return errors.New("Azure cost client and repository are required")
	}
	to := c.now().UTC().Truncate(24 * time.Hour).Add(24 * time.Hour)
	from := to.AddDate(0, 0, -90)
	scopes := c.scopes
	if len(scopes) == 0 && c.scope != "" {
		scopes = []string{c.scope}
	}
	if len(scopes) == 0 {
		return errors.New("Azure Cost Management has no configured scopes")
	}
	for _, scope := range scopes {
		if !validScope(scope) {
			return errors.New("Azure Cost Management contains an invalid configured scope")
		}
		client := *c
		client.scope = scope
		rows, err := client.query(ctx, from, to)
		if err != nil {
			return fmt.Errorf("Azure Cost Management scope query failed: %w", err)
		}
		if err := repo.ReplaceAzureActualCosts(ctx, scope, from, to, c.projectOrgs, rows); err != nil {
			return err
		}
	}
	return nil
}

func (c *Client) query(ctx context.Context, from, to time.Time) ([]Row, error) {
	token, err := c.accessToken(ctx)
	if err != nil {
		return nil, err
	}
	body := map[string]any{
		"type":       "ActualCost",
		"timeframe":  "Custom",
		"timePeriod": map[string]string{"from": from.Format(time.RFC3339), "to": to.Format(time.RFC3339)},
		"dataset": map[string]any{
			"granularity": "Daily",
			"aggregation": map[string]any{"totalCost": map[string]string{"name": "PreTaxCost", "function": "Sum"}},
			"grouping":    []map[string]string{{"name": "project", "type": "TagKey"}, {"name": "Meter", "type": "Dimension"}},
		},
	}
	encodedScope := strings.TrimRight(c.scope, "/")
	requestURL := managementHost + encodedScope + "/providers/Microsoft.CostManagement/query?api-version=" + apiVersion
	requestBody, _ := json.Marshal(body)
	rows := make([]Row, 0)
	totalBytes := 0
	for page := 0; page < maxPages; page++ {
		response, err := c.doQueryRequest(ctx, requestURL, requestBody, token)
		if err != nil {
			return nil, err
		}
		if response.StatusCode == http.StatusNoContent {
			response.Body.Close()
			return rows, nil
		}
		if response.StatusCode != http.StatusOK {
			status := response.StatusCode
			response.Body.Close()
			return nil, fmt.Errorf("Azure Cost Management query returned HTTP %d", status)
		}
		data, readErr := io.ReadAll(io.LimitReader(response.Body, maxResponse+1))
		response.Body.Close()
		if readErr != nil || len(data) > maxResponse {
			return nil, errors.New("Azure Cost Management response was unreadable or too large")
		}
		totalBytes += len(data)
		if totalBytes > maxTotalResponse {
			return nil, errors.New("Azure Cost Management query exceeded the total response size limit")
		}
		pageRows, nextLink, err := decodeRows(c.scope, data)
		if err != nil {
			return nil, err
		}
		rows = append(rows, pageRows...)
		if nextLink == "" {
			return rows, nil
		}
		next, err := url.Parse(nextLink)
		if err != nil || next.Scheme != "https" || !strings.EqualFold(next.Host, "management.azure.com") {
			return nil, errors.New("Azure Cost Management returned an unsafe pagination URL")
		}
		requestURL = next.String()
	}
	return nil, errors.New("Azure Cost Management query exceeded the pagination limit")
}

// doQueryRequest retries throttled and transient service responses using the
// retry guidance returned by Cost Management. A new request is built per try
// because net/http consumes request bodies after sending them.
func (c *Client) doQueryRequest(ctx context.Context, endpoint string, body []byte, token string) (*http.Response, error) {
	for attempt := 0; attempt < maxQueryAttempts; attempt++ {
		request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(body))
		if err != nil {
			return nil, errors.New("create Azure Cost Management query request")
		}
		request.Header.Set("Authorization", "Bearer "+token)
		request.Header.Set("Content-Type", "application/json")
		response, err := c.http.Do(request)
		if err != nil {
			return nil, fmt.Errorf("Azure Cost Management query transport failed: %w", err)
		}
		if response.StatusCode == http.StatusOK || response.StatusCode == http.StatusNoContent {
			return response, nil
		}

		status := response.StatusCode
		delay := costManagementRetryDelay(response.Header, attempt)
		response.Body.Close()
		if !retryableCostStatus(status) || attempt == maxQueryAttempts-1 {
			return nil, fmt.Errorf("Azure Cost Management query returned HTTP %d", status)
		}
		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil, ctx.Err()
		case <-timer.C:
		}
	}
	return nil, errors.New("Azure Cost Management query retry limit exceeded")
}

func retryableCostStatus(status int) bool {
	return status == http.StatusTooManyRequests || status == http.StatusServiceUnavailable || status == http.StatusBadGateway || status == http.StatusGatewayTimeout || status == http.StatusInternalServerError
}

func costManagementRetryDelay(headers http.Header, attempt int) time.Duration {
	for _, name := range []string{"x-ms-ratelimit-microsoft.consumption-retry-after", "Retry-After"} {
		if raw := strings.TrimSpace(headers.Get(name)); raw != "" {
			if seconds, err := strconv.ParseFloat(raw, 64); err == nil && seconds >= 0 {
				return min(time.Duration(seconds*float64(time.Second)), maxRetryDelay)
			}
			if retryAt, err := http.ParseTime(raw); err == nil {
				if delay := time.Until(retryAt); delay > 0 {
					return min(delay, maxRetryDelay)
				}
				return 0
			}
		}
	}
	// Exponential fallback for transient responses without a retry header.
	return time.Second * time.Duration(1<<attempt)
}

func (c *Client) accessToken(ctx context.Context) (string, error) {
	form := url.Values{
		"grant_type":    {"client_credentials"},
		"client_id":     {c.clientID},
		"client_secret": {c.clientSecret},
		"scope":         {"https://management.azure.com/.default"},
	}
	endpoint := "https://login.microsoftonline.com/" + url.PathEscape(c.tenantID) + "/oauth2/v2.0/token"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, strings.NewReader(form.Encode()))
	if err != nil {
		return "", errors.New("create Azure token request")
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	resp, err := c.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("Azure token request transport failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("Azure token endpoint returned HTTP %d", resp.StatusCode)
	}
	var result struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&result); err != nil || strings.TrimSpace(result.AccessToken) == "" {
		return "", errors.New("Azure token response did not contain an access token")
	}
	return result.AccessToken, nil
}

func decodeRows(scope string, data []byte) ([]Row, string, error) {
	var response struct {
		Properties struct {
			Columns []struct {
				Name string `json:"name"`
			} `json:"columns"`
			Rows     [][]any `json:"rows"`
			NextLink string  `json:"nextLink"`
		} `json:"properties"`
	}
	if err := json.Unmarshal(data, &response); err != nil {
		return nil, "", fmt.Errorf("decode Azure Cost Management response: %w", err)
	}
	// Azure may omit dimensions from an empty result. It is a valid snapshot and
	// should replace the scope's prior rows with an empty set.
	if len(response.Properties.Rows) == 0 {
		return []Row{}, response.Properties.NextLink, nil
	}
	indices := map[string]int{}
	columnNames := make([]string, 0, len(response.Properties.Columns))
	for i, column := range response.Properties.Columns {
		name := strings.TrimSpace(column.Name)
		columnNames = append(columnNames, name)
		indices[strings.ToLower(name)] = i
	}
	projectIndex, projectOK := indices["project"]
	tagKeyIndex, tagKeyOK := indices["tagkey"]
	tagValueIndex, tagValueOK := indices["tagvalue"]
	meterIndex, meterOK := indices["meter"]
	costIndex, costOK := indices["totalcost"]
	if !costOK {
		costIndex, costOK = indices["pretaxcost"]
	}
	currencyIndex, currencyOK := indices["currency"]
	dateIndex, dateOK := indices["usagedate"]
	if (!projectOK && !(tagKeyOK && tagValueOK)) || !meterOK || !costOK || !currencyOK || !dateOK {
		return nil, "", fmt.Errorf("Azure cost response lacks required project, meter, usage-date, cost, or currency columns (received: %s)", strings.Join(columnNames, ", "))
	}
	rows := make([]Row, 0, len(response.Properties.Rows))
	for _, values := range response.Properties.Rows {
		get := func(i int) (any, bool) {
			if i < 0 || i >= len(values) {
				return nil, false
			}
			return values[i], true
		}
		project := untaggedProject
		if projectOK {
			projectValue, ok := get(projectIndex)
			if !ok {
				return nil, "", errors.New("Azure cost row is missing its project tag")
			}
			project = strings.TrimSpace(fmt.Sprint(projectValue))
		} else {
			tagKeyValue, keyOK := get(tagKeyIndex)
			tagValue, valueOK := get(tagValueIndex)
			if !keyOK || !valueOK {
				return nil, "", errors.New("Azure cost row is missing its project tag")
			}
			// The API emits TagKey/TagValue for TagKey groupings. Accept only
			// the configured Foundry project key; never attribute another tag.
			if strings.EqualFold(strings.TrimSpace(fmt.Sprint(tagKeyValue)), "project") {
				project = strings.TrimSpace(fmt.Sprint(tagValue))
			}
		}
		meterValue, ok := get(meterIndex)
		if !ok {
			return nil, "", errors.New("Azure cost row is missing its meter")
		}
		currencyValue, ok := get(currencyIndex)
		if !ok {
			return nil, "", errors.New("Azure cost row is missing its currency")
		}
		dateValue, ok := get(dateIndex)
		if !ok {
			return nil, "", errors.New("Azure cost row is missing its usage date")
		}
		costValue, ok := get(costIndex)
		if !ok {
			return nil, "", errors.New("Azure cost row is missing its cost")
		}
		meter, currency := strings.TrimSpace(fmt.Sprint(meterValue)), strings.ToUpper(strings.TrimSpace(fmt.Sprint(currencyValue)))
		if project == "<nil>" || strings.EqualFold(project, "(not set)") {
			project = untaggedProject
		}
		if project == "" {
			project = untaggedProject
		}
		if meter == "" || meter == "<nil>" || strings.EqualFold(meter, "(not set)") {
			meter = unspecifiedMeter
		}
		if len(currency) != 3 {
			return nil, "", errors.New("Azure cost row is missing a valid currency code")
		}
		amount, err := numeric(costValue)
		if err != nil || math.IsNaN(amount) || math.IsInf(amount, 0) {
			return nil, "", errors.New("Azure cost row has invalid amount")
		}
		date, err := parseUsageDate(dateValue)
		if err != nil {
			return nil, "", err
		}
		rows = append(rows, Row{Scope: scope, Project: project, Meter: meter, Date: date, Amount: amount, Currency: currency})
	}
	return rows, response.Properties.NextLink, nil
}

func numeric(value any) (float64, error) {
	switch v := value.(type) {
	case float64:
		return v, nil
	case string:
		return strconv.ParseFloat(v, 64)
	default:
		return 0, errors.New("not numeric")
	}
}

func parseUsageDate(value any) (time.Time, error) {
	var raw string
	switch v := value.(type) {
	case string:
		raw = v
	case float64:
		raw = strconv.FormatInt(int64(v), 10)
	default:
		return time.Time{}, errors.New("Azure cost row has invalid usage date")
	}
	for _, layout := range []string{"20060102", "2006-01-02", time.RFC3339} {
		if parsed, err := time.Parse(layout, raw); err == nil {
			return parsed.UTC().Truncate(24 * time.Hour), nil
		}
	}
	return time.Time{}, errors.New("Azure cost row has unsupported usage date format")
}
