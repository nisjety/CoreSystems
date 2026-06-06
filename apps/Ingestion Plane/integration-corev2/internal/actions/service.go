package actions

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

const defaultActionTimeout = 15 * time.Second

type Service struct {
	cfg        config.Config
	httpClient *http.Client
}

type ExecuteInput struct {
	Connection  store.Connection
	AccessToken string
	Operation   string
	Params      map[string]any
	Body        map[string]any
}

type ExecuteResult struct {
	ProviderKey string `json:"providerKey"`
	Operation   string `json:"operation"`
	Result      any    `json:"result"`
}

func NewService(cfg config.Config, httpClient *http.Client) *Service {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: defaultActionTimeout}
	}
	return &Service{cfg: cfg, httpClient: httpClient}
}

func (s *Service) Execute(ctx context.Context, input ExecuteInput) (ExecuteResult, error) {
	operation := strings.TrimSpace(input.Operation)
	if operation == "" {
		return ExecuteResult{}, fmt.Errorf("operation is required")
	}
	var result any
	var err error
	switch input.Connection.ProviderKey {
	case "microsoft":
		result, err = s.executeMicrosoft(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "slack":
		result, err = s.executeSlack(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "google":
		result, err = s.executeGoogle(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "github":
		result, err = s.executeGitHub(ctx, input.AccessToken, operation, input.Params)
	case "notion":
		result, err = s.executeNotion(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "shopify":
		result, err = s.executeShopify(ctx, input.Connection.ProviderContext, input.AccessToken, operation, input.Params)
	case "stripe":
		result, err = s.executeStripe(ctx, input.AccessToken, operation, input.Params)
	case "okta":
		result, err = s.executeOkta(ctx, input.AccessToken, operation, input.Params)
	default:
		err = fmt.Errorf("actions are not implemented for provider %s", input.Connection.ProviderKey)
	}
	if err != nil {
		return ExecuteResult{}, err
	}
	return ExecuteResult{ProviderKey: input.Connection.ProviderKey, Operation: operation, Result: result}, nil
}

func (s *Service) executeMicrosoft(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.MicrosoftGraphBaseURL, "/") + "/v1.0"
	switch operation {
	case "profile", "microsoft.profile":
		return s.getBearer(ctx, token, base+"/me?$select=id,displayName,mail,userPrincipalName", nil)
	case "calendar.events", "microsoft.calendar.events":
		values := url.Values{"$top": {limitParam(params, "maxResults", 10, 100)}}
		return s.getBearer(ctx, token, base+"/me/events?"+values.Encode(), nil)
	case "mail.messages", "microsoft.mail.messages":
		values := url.Values{
			"$top":    {limitParam(params, "maxResults", 10, 100)},
			"$select": {"id,subject,from,receivedDateTime,webLink"},
		}
		return s.getBearer(ctx, token, base+"/me/messages?"+values.Encode(), nil)
	case "drive.files", "microsoft.drive.files":
		values := url.Values{
			"$top":    {limitParam(params, "maxResults", 10, 100)},
			"$select": {"id,name,webUrl,file,folder,lastModifiedDateTime"},
		}
		return s.getBearer(ctx, token, base+"/me/drive/root/children?"+values.Encode(), nil)
	case "mail.send", "microsoft.mail.send":
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for mail.send")
		}
		return s.postBearer(ctx, token, base+"/me/sendMail", body, nil)
	default:
		return nil, fmt.Errorf("unsupported Microsoft operation %q", operation)
	}
}

func (s *Service) executeSlack(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.SlackAPIBaseURL, "/")
	switch operation {
	case "channels.list", "slack.channels.list":
		values := url.Values{
			"types":            {stringParam(params, "types", "public_channel,private_channel")},
			"limit":            {limitParam(params, "limit", 100, 1000)},
			"exclude_archived": {"true"},
		}
		return s.getBearer(ctx, token, base+"/conversations.list?"+values.Encode(), nil)
	case "user", "slack.user", "users.info", "slack.users.info":
		userID := stringParam(params, "user", "")
		if userID == "" {
			userID = stringParam(params, "userId", "")
		}
		if userID == "" {
			return nil, fmt.Errorf("user is required")
		}
		values := url.Values{"user": {userID}}
		return s.getBearer(ctx, token, base+"/users.info?"+values.Encode(), nil)
	case "users.list", "slack.users.list":
		values := url.Values{"limit": {limitParam(params, "limit", 100, 1000)}}
		return s.getBearer(ctx, token, base+"/users.list?"+values.Encode(), nil)
	case "messages.list", "slack.messages.list":
		channel := stringParam(params, "channel", "")
		if channel == "" {
			return nil, fmt.Errorf("channel is required")
		}
		values := url.Values{"channel": {channel}, "limit": {limitParam(params, "limit", 50, 200)}}
		return s.getBearer(ctx, token, base+"/conversations.history?"+values.Encode(), nil)
	case "message.send", "slack.message.send":
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for message.send")
		}
		return s.postBearer(ctx, token, base+"/chat.postMessage", body, nil)
	default:
		return nil, fmt.Errorf("unsupported Slack operation %q", operation)
	}
}

func (s *Service) executeGoogle(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.GoogleAPIBaseURL, "/")
	switch operation {
	case "profile", "google.profile":
		return s.getBearer(ctx, token, base+"/oauth2/v3/userinfo", nil)
	case "gmail.messages", "google.gmail.messages":
		values := url.Values{"maxResults": {limitParam(params, "maxResults", 10, 100)}}
		return s.getBearer(ctx, token, base+"/gmail/v1/users/me/messages?"+values.Encode(), nil)
	case "gmail.send", "google.gmail.send":
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for gmail.send")
		}
		return s.postBearer(ctx, token, base+"/gmail/v1/users/me/messages/send", body, nil)
	case "calendar.events", "google.calendar.events":
		values := url.Values{"maxResults": {limitParam(params, "maxResults", 10, 100)}}
		return s.getBearer(ctx, token, base+"/calendar/v3/calendars/primary/events?"+values.Encode(), nil)
	case "drive.files", "google.drive.files":
		values := url.Values{
			"pageSize": {limitParam(params, "maxResults", 10, 100)},
			"fields":   {"files(id,name,mimeType,webViewLink,modifiedTime)"},
		}
		return s.getBearer(ctx, token, base+"/drive/v3/files?"+values.Encode(), nil)
	default:
		return nil, fmt.Errorf("unsupported Google operation %q", operation)
	}
}

func (s *Service) executeGitHub(ctx context.Context, token, operation string, params map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.GitHubAPIBaseURL, "/")
	headers := map[string]string{
		"Accept":               "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	}
	switch operation {
	case "user", "github.user":
		return s.getBearer(ctx, token, base+"/user", headers)
	case "orgs", "github.orgs":
		return s.getBearer(ctx, token, base+"/user/orgs", headers)
	case "teams", "github.teams":
		org := stringParam(params, "org", "")
		if org == "" {
			return nil, fmt.Errorf("org is required")
		}
		values := url.Values{"per_page": {limitParam(params, "perPage", 30, 100)}}
		return s.getBearer(ctx, token, base+"/orgs/"+url.PathEscape(org)+"/teams?"+values.Encode(), headers)
	case "repos", "github.repos":
		values := url.Values{
			"per_page": {limitParam(params, "perPage", 30, 100)},
			"sort":     {stringParam(params, "sort", "updated")},
			"type":     {stringParam(params, "type", "owner")},
		}
		if org := stringParam(params, "org", ""); org != "" {
			return s.getBearer(ctx, token, base+"/orgs/"+url.PathEscape(org)+"/repos?"+values.Encode(), headers)
		}
		return s.getBearer(ctx, token, base+"/user/repos?"+values.Encode(), headers)
	case "repo", "github.repo":
		owner := stringParam(params, "owner", "")
		repo := stringParam(params, "repo", "")
		if owner == "" || repo == "" {
			return nil, fmt.Errorf("owner and repo are required")
		}
		return s.getBearer(ctx, token, base+"/repos/"+url.PathEscape(owner)+"/"+url.PathEscape(repo), headers)
	default:
		return nil, fmt.Errorf("unsupported GitHub operation %q", operation)
	}
}

func (s *Service) executeNotion(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.NotionAPIBaseURL, "/")
	headers := map[string]string{"Notion-Version": "2022-06-28"}
	switch operation {
	case "user", "notion.user":
		return s.getBearer(ctx, token, base+"/v1/users/me", headers)
	case "databases", "notion.databases":
		payload := map[string]any{"filter": map[string]any{"value": "database", "property": "object"}}
		return s.postBearer(ctx, token, base+"/v1/search", payload, headers)
	case "pages", "notion.pages":
		databaseID := stringParam(params, "databaseId", "")
		if databaseID != "" {
			payload := body
			if payload == nil {
				payload = map[string]any{}
			}
			return s.postBearer(ctx, token, base+"/v1/databases/"+url.PathEscape(databaseID)+"/query", payload, headers)
		}
		payload := map[string]any{"filter": map[string]any{"value": "page", "property": "object"}}
		return s.postBearer(ctx, token, base+"/v1/search", payload, headers)
	default:
		return nil, fmt.Errorf("unsupported Notion operation %q", operation)
	}
}

func (s *Service) executeShopify(ctx context.Context, providerContext map[string]string, token, operation string, params map[string]any) (any, error) {
	shop, err := oauth.ShopifyShop(providerContext)
	if err != nil {
		return nil, err
	}
	base := "https://" + shop + "/admin/api/2026-01"
	switch operation {
	case "shop", "shopify.shop":
		return s.getShopify(ctx, token, base+"/shop.json")
	case "products", "shopify.products":
		values := url.Values{"limit": {limitParam(params, "limit", 50, 250)}}
		return s.getShopify(ctx, token, base+"/products.json?"+values.Encode())
	case "orders", "shopify.orders":
		values := url.Values{
			"limit":  {limitParam(params, "limit", 50, 250)},
			"status": {stringParam(params, "status", "any")},
		}
		return s.getShopify(ctx, token, base+"/orders.json?"+values.Encode())
	default:
		return nil, fmt.Errorf("unsupported Shopify operation %q", operation)
	}
}

func (s *Service) executeStripe(ctx context.Context, token, operation string, params map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.StripeAPIBaseURL, "/")
	switch operation {
	case "account", "stripe.account":
		return s.getBearer(ctx, token, base+"/v1/account", nil)
	case "customers", "stripe.customers":
		values := url.Values{"limit": {limitParam(params, "limit", 10, 100)}}
		return s.getBearer(ctx, token, base+"/v1/customers?"+values.Encode(), nil)
	case "subscriptions", "stripe.subscriptions":
		values := url.Values{"limit": {limitParam(params, "limit", 10, 100)}}
		return s.getBearer(ctx, token, base+"/v1/subscriptions?"+values.Encode(), nil)
	case "invoices", "stripe.invoices":
		values := url.Values{"limit": {limitParam(params, "limit", 10, 100)}}
		return s.getBearer(ctx, token, base+"/v1/invoices?"+values.Encode(), nil)
	default:
		return nil, fmt.Errorf("unsupported Stripe operation %q", operation)
	}
}

func (s *Service) executeOkta(ctx context.Context, token, operation string, params map[string]any) (any, error) {
	base := strings.TrimRight(firstNonEmpty(s.cfg.OktaAPIBaseURL, s.cfg.OktaDomain), "/")
	if base == "" {
		return nil, fmt.Errorf("Okta API base URL is required")
	}
	headers, err := s.oktaHeaders(token)
	if err != nil {
		return nil, err
	}
	switch operation {
	case "org", "okta.org":
		return s.get(ctx, base+"/api/v1/org", headers)
	case "users", "okta.users":
		values := url.Values{"limit": {limitParam(params, "limit", 50, 200)}}
		return s.get(ctx, base+"/api/v1/users?"+values.Encode(), headers)
	case "groups", "okta.groups":
		values := url.Values{"limit": {limitParam(params, "limit", 50, 200)}}
		return s.get(ctx, base+"/api/v1/groups?"+values.Encode(), headers)
	case "user.suspend", "okta.user.suspend":
		userID := stringParam(params, "userId", "")
		if userID == "" {
			return nil, fmt.Errorf("userId is required")
		}
		return s.post(ctx, base+"/api/v1/users/"+url.PathEscape(userID)+"/lifecycle/suspend", nil, headers)
	case "user.activate", "okta.user.activate":
		userID := stringParam(params, "userId", "")
		if userID == "" {
			return nil, fmt.Errorf("userId is required")
		}
		values := url.Values{"sendEmail": {stringParam(params, "sendEmail", "false")}}
		return s.post(ctx, base+"/api/v1/users/"+url.PathEscape(userID)+"/lifecycle/activate?"+values.Encode(), nil, headers)
	default:
		return nil, fmt.Errorf("unsupported Okta operation %q", operation)
	}
}

func (s *Service) getBearer(ctx context.Context, token, endpoint string, headers map[string]string) (any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("build provider request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	return s.doJSON(req)
}

func (s *Service) get(ctx context.Context, endpoint string, headers map[string]string) (any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("build provider request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	return s.doJSON(req)
}

func (s *Service) post(ctx context.Context, endpoint string, body map[string]any, headers map[string]string) (any, error) {
	var reader *bytes.Reader
	if len(body) == 0 {
		reader = bytes.NewReader(nil)
	} else {
		payload, err := json.Marshal(body)
		if err != nil {
			return nil, fmt.Errorf("marshal provider request: %w", err)
		}
		reader = bytes.NewReader(payload)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, reader)
	if err != nil {
		return nil, fmt.Errorf("build provider request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	if len(body) > 0 {
		req.Header.Set("Content-Type", "application/json")
	}
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	return s.doJSON(req)
}

func (s *Service) postBearer(ctx context.Context, token, endpoint string, body map[string]any, headers map[string]string) (any, error) {
	payload, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("marshal provider request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("build provider request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	return s.doJSON(req)
}

func (s *Service) oktaHeaders(token string) (map[string]string, error) {
	token = strings.TrimSpace(firstNonEmpty(token, s.cfg.OktaAPIToken))
	if token == "" {
		return nil, fmt.Errorf("Okta token is required")
	}
	if strings.TrimSpace(s.cfg.OktaAPIToken) != "" && token == strings.TrimSpace(s.cfg.OktaAPIToken) {
		return map[string]string{"Authorization": "SSWS " + token}, nil
	}
	return map[string]string{"Authorization": "Bearer " + token}, nil
}

func (s *Service) getShopify(ctx context.Context, token, endpoint string) (any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("build Shopify request: %w", err)
	}
	req.Header.Set("X-Shopify-Access-Token", token)
	req.Header.Set("Accept", "application/json")
	return s.doJSON(req)
}

func (s *Service) doJSON(req *http.Request) (any, error) {
	resp, err := s.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call provider action endpoint: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNoContent {
		return map[string]any{"ok": true}, nil
	}
	var decoded any
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		return nil, fmt.Errorf("decode provider action response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("provider action endpoint returned status %d", resp.StatusCode)
	}
	return decoded, nil
}

func stringParam(params map[string]any, key, fallback string) string {
	value, ok := params[key]
	if !ok {
		return fallback
	}
	if text, ok := value.(string); ok && strings.TrimSpace(text) != "" {
		return strings.TrimSpace(text)
	}
	return fallback
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}

func limitParam(params map[string]any, key string, fallback, max int) string {
	value, ok := params[key]
	if !ok {
		return fmt.Sprintf("%d", fallback)
	}
	limit := fallback
	switch typed := value.(type) {
	case float64:
		limit = int(typed)
	case int:
		limit = typed
	case string:
		_, _ = fmt.Sscanf(typed, "%d", &limit)
	}
	if limit <= 0 {
		limit = fallback
	}
	if limit > max {
		limit = max
	}
	return fmt.Sprintf("%d", limit)
}
