package actions

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
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
		result, err = s.executeGitHub(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "notion":
		result, err = s.executeNotion(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "shopify":
		result, err = s.executeShopify(ctx, input.Connection.ProviderContext, input.AccessToken, operation, input.Params)
	case "stripe":
		result, err = s.executeStripe(ctx, input.AccessToken, operation, input.Params)
	case "linkedin":
		result, err = s.executeLinkedIn(ctx, input.AccessToken, operation, input.Params, input.Body)
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		result, err = s.executeMeta(ctx, input.Connection.ProviderKey, input.AccessToken, operation, input.Params, input.Body)
	case "snapchat":
		result, err = s.executeSnapchat(ctx, input.AccessToken, operation, input.Params, input.Body)
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

func (s *Service) executeGitHub(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.GitHubAPIBaseURL, "/")
	headers := map[string]string{
		"Accept":               "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	}
	switch operation {
	case "user", "github.user":
		return s.getBearer(ctx, token, base+"/user", headers)
	case "emails", "github.emails":
		values := url.Values{"per_page": {limitParam(params, "perPage", 30, 100)}}
		return s.getBearer(ctx, token, base+"/user/emails?"+values.Encode(), headers)
	case "orgs", "github.orgs":
		values := url.Values{"per_page": {limitParam(params, "perPage", 30, 100)}}
		return s.getBearer(ctx, token, base+"/user/orgs?"+values.Encode(), headers)
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
	case "contents.get", "github.contents.get":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		contentPath, err := githubContentPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{}
		if ref := stringParam(params, "ref", ""); ref != "" {
			values.Set("ref", ref)
		}
		endpoint := base + repoPath + "/contents/" + contentPath
		if encoded := values.Encode(); encoded != "" {
			endpoint += "?" + encoded
		}
		return s.getBearer(ctx, token, endpoint, headers)
	case "readme.get", "github.readme.get":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{}
		if ref := stringParam(params, "ref", ""); ref != "" {
			values.Set("ref", ref)
		}
		endpoint := base + repoPath + "/readme"
		if encoded := values.Encode(); encoded != "" {
			endpoint += "?" + encoded
		}
		return s.getBearer(ctx, token, endpoint, headers)
	case "branches", "github.branches":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{"per_page": {limitParam(params, "perPage", 30, 100)}}
		return s.getBearer(ctx, token, base+repoPath+"/branches?"+values.Encode(), headers)
	case "commits", "github.commits":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{"per_page": {limitParam(params, "perPage", 30, 100)}}
		if sha := stringParam(params, "sha", ""); sha != "" {
			values.Set("sha", sha)
		}
		return s.getBearer(ctx, token, base+repoPath+"/commits?"+values.Encode(), headers)
	case "pulls", "pulls.list", "github.pulls", "github.pulls.list":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"per_page": {limitParam(params, "perPage", 30, 100)},
			"state":    {stringParam(params, "state", "open")},
		}
		return s.getBearer(ctx, token, base+repoPath+"/pulls?"+values.Encode(), headers)
	case "issues", "issues.list", "github.issues", "github.issues.list":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"per_page": {limitParam(params, "perPage", 30, 100)},
			"state":    {stringParam(params, "state", "open")},
		}
		return s.getBearer(ctx, token, base+repoPath+"/issues?"+values.Encode(), headers)
	case "issues.create", "github.issues.create":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for issues.create")
		}
		return s.postBearer(ctx, token, base+repoPath+"/issues", body, headers)
	case "issues.update", "github.issues.update":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		issueNumber, err := requiredStringParam(params, "issueNumber")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for issues.update")
		}
		return s.patchBearer(ctx, token, base+repoPath+"/issues/"+url.PathEscape(issueNumber), body, headers)
	case "issues.comment.create", "github.issues.comment.create":
		repoPath, err := githubRepoPath(params)
		if err != nil {
			return nil, err
		}
		issueNumber, err := requiredStringParam(params, "issueNumber")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for issues.comment.create")
		}
		return s.postBearer(ctx, token, base+repoPath+"/issues/"+url.PathEscape(issueNumber)+"/comments", body, headers)
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

func (s *Service) executeLinkedIn(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	base := strings.TrimRight(s.cfg.LinkedInAPIBaseURL, "/")
	headers := s.linkedinHeaders()
	switch strings.TrimSpace(strings.ToLower(operation)) {
	case "profile", "linkedin.profile":
		return s.getBearer(ctx, token, base+"/v2/userinfo", nil)
	case "identity", "linkedin.identity":
		return s.getBearer(ctx, token, base+"/rest/identityMe", headers)
	case "verification.report", "linkedin.verification.report":
		values := url.Values{}
		for _, criteria := range stringSliceParam(params, "verificationCriteria") {
			values.Add("verificationCriteria", criteria)
		}
		endpoint := base + "/rest/verificationReport"
		if encoded := values.Encode(); encoded != "" {
			endpoint += "?" + encoded
		}
		return s.getBearer(ctx, token, endpoint, headers)
	case "organization.acls", "organizations", "linkedin.organization.acls":
		values := url.Values{
			"q":     {"roleAssignee"},
			"count": {limitParam(params, "count", 10, 100)},
			"start": {stringParam(params, "start", "0")},
		}
		if role := stringParam(params, "role", ""); role != "" {
			values.Set("role", role)
		}
		if state := stringParam(params, "state", ""); state != "" {
			values.Set("state", state)
		}
		return s.getBearer(ctx, token, base+"/rest/organizationAcls?"+values.Encode(), headers)
	case "posts.list", "linkedin.posts.list":
		author, err := requiredStringParam(params, "author")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"q":      {"author"},
			"author": {author},
			"count":  {limitParam(params, "count", 10, 100)},
			"start":  {stringParam(params, "start", "0")},
		}
		return s.getBearer(ctx, token, base+"/rest/posts?"+values.Encode(), headers)
	case "posts.create", "linkedin.posts.create":
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for posts.create")
		}
		return s.postBearer(ctx, token, base+"/rest/posts", body, headers)
	case "events.create", "linkedin.events.create":
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for events.create")
		}
		return s.postBearer(ctx, token, base+"/rest/events", body, headers)
	case "events.get", "linkedin.events.get":
		eventID, err := requiredStringParam(params, "eventId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, base+"/rest/events/"+url.PathEscape(eventID), headers)
	case "events.update", "linkedin.events.update":
		eventID, err := requiredStringParam(params, "eventId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for events.update")
		}
		updateHeaders := copyHeaders(headers)
		updateHeaders["X-RestLi-Method"] = "partial_update"
		return s.postBearer(ctx, token, base+"/rest/events/"+url.PathEscape(eventID), body, updateHeaders)
	case "ads.accounts", "linkedin.ads.accounts":
		values := url.Values{"pageSize": {limitParam(params, "pageSize", 10, 100)}}
		if pageToken := stringParam(params, "pageToken", ""); pageToken != "" {
			values.Set("pageToken", pageToken)
		}
		return s.getBearer(ctx, token, base+"/rest/adAccounts?"+values.Encode(), headers)
	case "ads.account", "linkedin.ads.account":
		accountID, err := requiredStringParam(params, "accountId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, base+"/rest/adAccounts/"+url.PathEscape(accountID), headers)
	case "ads.campaigns", "linkedin.ads.campaigns":
		accountID, err := requiredStringParam(params, "accountId")
		if err != nil {
			return nil, err
		}
		values := url.Values{"pageSize": {limitParam(params, "pageSize", 10, 100)}}
		if search := stringParam(params, "search", ""); search != "" {
			values.Set("q", "search")
			values.Set("search", search)
		}
		if sortOrder := stringParam(params, "sortOrder", ""); sortOrder != "" {
			values.Set("sortOrder", sortOrder)
		}
		if pageToken := stringParam(params, "pageToken", ""); pageToken != "" {
			values.Set("pageToken", pageToken)
		}
		return s.getBearer(ctx, token, base+"/rest/adAccounts/"+url.PathEscape(accountID)+"/adCampaigns?"+values.Encode(), headers)
	case "ads.campaign", "linkedin.ads.campaign":
		accountID, err := requiredStringParam(params, "accountId")
		if err != nil {
			return nil, err
		}
		campaignID, err := requiredStringParam(params, "campaignId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, base+"/rest/adAccounts/"+url.PathEscape(accountID)+"/adCampaigns/"+url.PathEscape(campaignID), headers)
	case "ads.campaign.create", "linkedin.ads.campaign.create":
		accountID, err := requiredStringParam(params, "accountId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for ads.campaign.create")
		}
		return s.postBearer(ctx, token, base+"/rest/adAccounts/"+url.PathEscape(accountID)+"/adCampaigns", body, headers)
	case "ads.campaign.update", "linkedin.ads.campaign.update":
		accountID, err := requiredStringParam(params, "accountId")
		if err != nil {
			return nil, err
		}
		campaignID, err := requiredStringParam(params, "campaignId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for ads.campaign.update")
		}
		updateHeaders := copyHeaders(headers)
		updateHeaders["X-RestLi-Method"] = "partial_update"
		return s.postBearer(ctx, token, base+"/rest/adAccounts/"+url.PathEscape(accountID)+"/adCampaigns/"+url.PathEscape(campaignID), body, updateHeaders)
	case "conversions.list", "linkedin.conversions.list":
		account, err := requiredStringParam(params, "account")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"q":       {"account"},
			"account": {account},
			"count":   {limitParam(params, "count", 10, 100)},
			"start":   {stringParam(params, "start", "0")},
		}
		return s.getBearer(ctx, token, base+"/rest/conversions?"+values.Encode(), headers)
	case "conversions.create", "linkedin.conversions.create":
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for conversions.create")
		}
		values := url.Values{}
		if autoAssociationType := stringParam(params, "autoAssociationType", ""); autoAssociationType != "" {
			values.Set("autoAssociationType", autoAssociationType)
		}
		endpoint := base + "/rest/conversions"
		if encoded := values.Encode(); encoded != "" {
			endpoint += "?" + encoded
		}
		return s.postBearer(ctx, token, endpoint, body, headers)
	case "lead.forms", "linkedin.lead.forms":
		owner, err := requiredStringParam(params, "owner")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"q":     {"owner"},
			"owner": {owner},
			"count": {limitParam(params, "count", 10, 100)},
			"start": {stringParam(params, "start", "0")},
		}
		return s.getBearer(ctx, token, base+"/rest/leadForms?"+values.Encode(), headers)
	case "lead.responses", "linkedin.lead.responses":
		leadForm, err := requiredStringParam(params, "leadForm")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"q":        {"leadForm"},
			"leadForm": {leadForm},
			"count":    {limitParam(params, "count", 10, 100)},
			"start":    {stringParam(params, "start", "0")},
		}
		return s.getBearer(ctx, token, base+"/rest/leadFormResponses?"+values.Encode(), headers)
	default:
		return nil, fmt.Errorf("unsupported LinkedIn operation %q", operation)
	}
}

func (s *Service) executeMeta(ctx context.Context, providerKey, token, operation string, params, body map[string]any) (any, error) {
	graphBase := strings.TrimRight(firstNonEmpty(s.cfg.FacebookAPIBaseURL, s.cfg.InstagramAPIBaseURL, "https://graph.facebook.com/v25.0"), "/")
	if providerKey == "instagram" && strings.TrimSpace(s.cfg.InstagramAPIBaseURL) != "" {
		graphBase = strings.TrimRight(s.cfg.InstagramAPIBaseURL, "/")
	}
	threadsBase := strings.TrimRight(firstNonEmpty(s.cfg.MetaThreadsAPIBaseURL, "https://graph.threads.net/v1.0"), "/")
	normalized := strings.TrimSpace(strings.ToLower(operation))
	switch normalized {
	case "profile", "meta.profile", "facebook.profile":
		return s.getBearer(ctx, token, graphBase+"/me?fields=id,name", nil)
	case "pages.list", "facebook.pages", "meta.pages":
		values := url.Values{
			"fields": {"id,name,category,tasks,instagram_business_account{id,username,name},connected_instagram_account{id,username}"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/me/accounts?"+values.Encode(), nil)
	case "pages.post", "facebook.page.post":
		pageID, err := requiredStringParam(params, "pageId")
		if err != nil {
			return nil, err
		}
		pageToken, err := s.metaPageAccessToken(ctx, graphBase, token, pageID)
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, pageToken, graphBase+"/"+url.PathEscape(pageID)+"/feed", valuesFromMap(body))
	case "pages.photo", "facebook.page.photo":
		pageID, err := requiredStringParam(params, "pageId")
		if err != nil {
			return nil, err
		}
		pageToken, err := s.metaPageAccessToken(ctx, graphBase, token, pageID)
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, pageToken, graphBase+"/"+url.PathEscape(pageID)+"/photos", valuesFromMap(body))
	case "live.create", "facebook.live.create":
		pageID, err := requiredStringParam(params, "pageId")
		if err != nil {
			return nil, err
		}
		pageToken, err := s.metaPageAccessToken(ctx, graphBase, token, pageID)
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, pageToken, graphBase+"/"+url.PathEscape(pageID)+"/live_videos", valuesFromMap(body))
	case "live.list", "facebook.live.list":
		pageID, err := requiredStringParam(params, "pageId")
		if err != nil {
			return nil, err
		}
		pageToken, err := s.metaPageAccessToken(ctx, graphBase, token, pageID)
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"fields": {"id,title,status,creation_time,permalink_url"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, pageToken, graphBase+"/"+url.PathEscape(pageID)+"/live_videos?"+values.Encode(), nil)
	case "live.get", "facebook.live.get":
		liveVideoID, err := requiredStringParam(params, "liveVideoId")
		if err != nil {
			return nil, err
		}
		values := url.Values{"fields": {"id,title,status,stream_url,secure_stream_url,embed_html,permalink_url"}}
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(liveVideoID)+"?"+values.Encode(), nil)
	case "instagram.accounts", "meta.instagram.accounts":
		values := url.Values{
			"fields": {"id,name,instagram_business_account{id,username,name,profile_picture_url}"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/me/accounts?"+values.Encode(), nil)
	case "instagram.media.create":
		igUserID, err := requiredStringParam(params, "igUserId")
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, token, graphBase+"/"+url.PathEscape(igUserID)+"/media", valuesFromMap(body))
	case "instagram.media.publish":
		igUserID, err := requiredStringParam(params, "igUserId")
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, token, graphBase+"/"+url.PathEscape(igUserID)+"/media_publish", valuesFromMap(body))
	case "instagram.media.status":
		creationID, err := requiredStringParam(params, "creationId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(creationID)+"?fields=id,status_code,status", nil)
	case "instagram.insights":
		mediaID, err := requiredStringParam(params, "mediaId")
		if err != nil {
			return nil, err
		}
		metrics := stringParam(params, "metric", "impressions,reach,likes,comments,saved,shares")
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(mediaID)+"/insights?"+url.Values{"metric": {metrics}}.Encode(), nil)
	case "whatsapp.business_accounts", "whatsapp.accounts":
		values := url.Values{
			"fields": {"id,name,owned_whatsapp_business_accounts{id,name,currency,timezone_id}"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/me/businesses?"+values.Encode(), nil)
	case "whatsapp.phone_numbers":
		wabaID, err := requiredStringParam(params, "wabaId")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"fields": {"id,display_phone_number,verified_name,quality_rating"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(wabaID)+"/phone_numbers?"+values.Encode(), nil)
	case "whatsapp.templates":
		wabaID, err := requiredStringParam(params, "wabaId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(wabaID)+"/message_templates?"+url.Values{"limit": {limitParam(params, "limit", 25, 100)}}.Encode(), nil)
	case "whatsapp.messages.send":
		phoneNumberID, err := requiredStringParam(params, "phoneNumberId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for whatsapp.messages.send")
		}
		message, err := whatsAppMessagePayload(body)
		if err != nil {
			return nil, err
		}
		return s.postBearer(ctx, token, graphBase+"/"+url.PathEscape(phoneNumberID)+"/messages", message, nil)
	case "messenger.messages.send":
		pageID, err := requiredStringParam(params, "pageId")
		if err != nil {
			return nil, err
		}
		pageToken, err := s.metaPageAccessToken(ctx, graphBase, token, pageID)
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for messenger.messages.send")
		}
		return s.postBearer(ctx, pageToken, graphBase+"/"+url.PathEscape(pageID)+"/messages", body, nil)
	case "instagram.messages.send":
		// Instagram DMs send through the LINKED Facebook Page's /messages edge
		// (Messenger Platform) with the IGSID as recipient — there is no
		// IG-account-scoped send edge. Callers supply the IG business-account
		// id (what inbound webhooks carry); the page + page token are resolved
		// here.
		igAccountID, err := requiredStringParam(params, "igAccountId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for instagram.messages.send")
		}
		igPageID, igPageToken, err := s.metaPageForInstagramAccount(ctx, graphBase, token, igAccountID)
		if err != nil {
			return nil, err
		}
		return s.postBearer(ctx, igPageToken, graphBase+"/"+url.PathEscape(igPageID)+"/messages", body, nil)
	case "messenger.subscribed_apps":
		pageID, err := requiredStringParam(params, "pageId")
		if err != nil {
			return nil, err
		}
		pageToken, err := s.metaPageAccessToken(ctx, graphBase, token, pageID)
		if err != nil {
			return nil, err
		}
		values := url.Values{"subscribed_fields": {stringParam(params, "subscribedFields", "messages,messaging_postbacks,messaging_optins,message_deliveries,message_reads")}}
		return s.postBearerForm(ctx, pageToken, graphBase+"/"+url.PathEscape(pageID)+"/subscribed_apps", values)
	case "ads.businesses", "meta.businesses":
		return s.getBearer(ctx, token, graphBase+"/me/businesses?"+url.Values{"fields": {"id,name,verification_status"}, "limit": {limitParam(params, "limit", 25, 100)}}.Encode(), nil)
	case "audience_network.apps", "meta.audience_network.apps":
		return s.getBearer(ctx, token, graphBase+"/me/applications?"+url.Values{"fields": {"id,name,namespace,link"}, "limit": {limitParam(params, "limit", 25, 100)}}.Encode(), nil)
	case "ads.adaccounts", "meta.adaccounts":
		values := url.Values{
			"fields": {"id,account_id,name,account_status,currency,business"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/me/adaccounts?"+values.Encode(), nil)
	case "ads.campaigns":
		adAccountPath, err := adAccountPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"fields": {"id,name,status,effective_status,objective,buying_type,created_time,updated_time"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/"+adAccountPath+"/campaigns?"+values.Encode(), nil)
	case "ads.campaign.create", "app_ads.campaign.create":
		adAccountPath, err := adAccountPath(params)
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, token, graphBase+"/"+adAccountPath+"/campaigns", valuesFromMap(body))
	case "ads.adsets":
		adAccountPath, err := adAccountPath(params)
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, graphBase+"/"+adAccountPath+"/adsets?"+url.Values{"fields": {"id,name,status,effective_status,campaign_id,daily_budget,lifetime_budget"}, "limit": {limitParam(params, "limit", 25, 100)}}.Encode(), nil)
	case "ads.ads":
		adAccountPath, err := adAccountPath(params)
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, graphBase+"/"+adAccountPath+"/ads?"+url.Values{"fields": {"id,name,status,effective_status,campaign_id,adset_id,creative"}, "limit": {limitParam(params, "limit", 25, 100)}}.Encode(), nil)
	case "ads.creatives":
		adAccountPath, err := adAccountPath(params)
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, graphBase+"/"+adAccountPath+"/adcreatives?"+url.Values{"fields": {"id,name,status,thumbnail_url,object_story_spec"}, "limit": {limitParam(params, "limit", 25, 100)}}.Encode(), nil)
	case "ads.insights":
		adAccountPath, err := adAccountPath(params)
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"fields": {"campaign_id,campaign_name,impressions,reach,clicks,spend,actions"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		if datePreset := stringParam(params, "datePreset", ""); datePreset != "" {
			values.Set("date_preset", datePreset)
		}
		return s.getBearer(ctx, token, graphBase+"/"+adAccountPath+"/insights?"+values.Encode(), nil)
	case "catalogs.list", "catalog.list":
		businessID, err := requiredStringParam(params, "businessId")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"fields": {"id,name,vertical,product_count"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(businessID)+"/owned_product_catalogs?"+values.Encode(), nil)
	case "catalog.products":
		catalogID, err := requiredStringParam(params, "catalogId")
		if err != nil {
			return nil, err
		}
		values := url.Values{
			"fields": {"id,name,retailer_id,availability,price,currency,condition,updated_time"},
			"limit":  {limitParam(params, "limit", 25, 100)},
		}
		return s.getBearer(ctx, token, graphBase+"/"+url.PathEscape(catalogID)+"/products?"+values.Encode(), nil)
	case "catalog.product.upsert":
		catalogID, err := requiredStringParam(params, "catalogId")
		if err != nil {
			return nil, err
		}
		return s.postBearerForm(ctx, token, graphBase+"/"+url.PathEscape(catalogID)+"/products", valuesFromMap(body))
	case "catalog.batch":
		catalogID, err := requiredStringParam(params, "catalogId")
		if err != nil {
			return nil, err
		}
		return s.postBearer(ctx, token, graphBase+"/"+url.PathEscape(catalogID)+"/batch", body, nil)
	case "threads.profile":
		return s.getBearer(ctx, token, threadsBase+"/me?fields=id,username,name,threads_profile_picture_url,threads_biography", nil)
	case "threads.container.create":
		return s.postBearerForm(ctx, token, threadsBase+"/me/threads", valuesFromMap(body))
	case "threads.container.status":
		// Video/CAROUSEL containers process asynchronously, same as
		// Instagram's — calling threads.publish before status is FINISHED
		// fails. Mirrors instagram.media.status: the caller (agent tool
		// loop / scheduler) is responsible for polling this before
		// threads.publish, this action-execution layer does not block
		// internally for any operation.
		creationID, err := requiredStringParam(params, "creationId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, threadsBase+"/"+url.PathEscape(creationID)+"?fields=id,status,error_message", nil)
	case "threads.publish":
		return s.postBearerForm(ctx, token, threadsBase+"/me/threads_publish", valuesFromMap(body))
	case "threads.insights":
		return s.getBearer(ctx, token, threadsBase+"/me/threads_insights?"+url.Values{"metric": {stringParam(params, "metric", "views,likes,replies,reposts,quotes")}}.Encode(), nil)
	case "oembed", "meta.oembed":
		contentURL := firstNonEmpty(stringParam(params, "url", ""), stringParam(body, "url", ""))
		if contentURL == "" {
			return nil, fmt.Errorf("url is required for oembed")
		}
		endpoint, err := metaOEmbedEndpoint(stringParam(params, "kind", "post"))
		if err != nil {
			return nil, err
		}
		values := url.Values{"url": {contentURL}}
		if maxWidth := stringParam(params, "maxWidth", ""); maxWidth != "" {
			values.Set("maxwidth", maxWidth)
		}
		return s.getBearer(ctx, token, graphBase+"/"+endpoint+"?"+values.Encode(), nil)
	default:
		return nil, fmt.Errorf("unsupported Meta operation %q", operation)
	}
}

// executeSnapchat dispatches Snapchat Marketing API operations. Snapchat spans
// two hosts that share ONE OAuth (accounts.snapchat.com, scope
// snapchat-marketing-api):
//
//   - Ads/Marketing API (adsapi.snapchat.com, SnapchatAPIBaseURL): organizations,
//     ad accounts, ad-creative media containers, creatives, and reporting. This
//     is the ads-publishing path and is generally available to any approved
//     Snap Ads app.
//   - Public Profile API (businessapi.snapchat.com, SnapchatBusinessAPIBaseURL):
//     organic Story/Spotlight/Saved Story content management. This is
//     ALLOWLIST-ONLY — Snap must allowlist the OAuth app's client id, and
//     content management additionally needs a Partnership Role on the target
//     profile. See docs/actions-surface-operations.md.
//
// Both hosts take Bearer tokens and return JSON. Only the JSON write/read
// primitives live here: the raw media-BYTES upload (ads POST /media/{id}/upload,
// Public Profile multipart ADD/FINALIZE with client-side AES-256-CBC encryption)
// is a binary/multipart flow that does not fit this JSON action surface —
// social-core's publisher owns that pipeline. The operations below reference a
// media_id whose bytes were uploaded out of band.
func (s *Service) executeSnapchat(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	adsBase := strings.TrimRight(firstNonEmpty(s.cfg.SnapchatAPIBaseURL, "https://adsapi.snapchat.com/v1"), "/")
	profileBase := strings.TrimRight(firstNonEmpty(s.cfg.SnapchatBusinessAPIBaseURL, "https://businessapi.snapchat.com/v1"), "/")
	switch strings.TrimSpace(strings.ToLower(operation)) {
	// --- Ads / Marketing API (adsapi.snapchat.com) reads ---
	case "organizations", "snapchat.organizations":
		return s.getBearer(ctx, token, adsBase+"/me/organizations", nil)
	case "adaccounts", "snapchat.adaccounts":
		organizationID, err := requiredStringParam(params, "organizationId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, adsBase+"/organizations/"+url.PathEscape(organizationID)+"/adaccounts", nil)
	case "media.list", "snapchat.media.list":
		adAccountID, err := requiredStringParam(params, "adAccountId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, adsBase+"/adaccounts/"+url.PathEscape(adAccountID)+"/media", nil)
	case "creatives.list", "snapchat.creatives.list":
		adAccountID, err := requiredStringParam(params, "adAccountId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, adsBase+"/adaccounts/"+url.PathEscape(adAccountID)+"/creatives", nil)
	case "ads.stats", "snapchat.ads.stats":
		adAccountID, err := requiredStringParam(params, "adAccountId")
		if err != nil {
			return nil, err
		}
		values := url.Values{}
		for _, key := range []string{"granularity", "fields", "start_time", "end_time", "breakdown"} {
			if v := stringParam(params, key, ""); v != "" {
				values.Set(key, v)
			}
		}
		endpoint := adsBase + "/adaccounts/" + url.PathEscape(adAccountID) + "/stats"
		if encoded := values.Encode(); encoded != "" {
			endpoint += "?" + encoded
		}
		return s.getBearer(ctx, token, endpoint, nil)
	// --- Ads / Marketing API writes (JSON; media BYTES uploaded out of band) ---
	case "ads.media.create", "snapchat.ads.media.create":
		adAccountID, err := requiredStringParam(params, "adAccountId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for snapchat.ads.media.create")
		}
		return s.postBearer(ctx, token, adsBase+"/adaccounts/"+url.PathEscape(adAccountID)+"/media", body, nil)
	case "ads.creative.create", "snapchat.ads.creative.create":
		adAccountID, err := requiredStringParam(params, "adAccountId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for snapchat.ads.creative.create")
		}
		return s.postBearer(ctx, token, adsBase+"/adaccounts/"+url.PathEscape(adAccountID)+"/creatives", body, nil)
	// --- Public Profile API (businessapi.snapchat.com) reads ---
	case "profile.stories", "snapchat.profile.stories":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/stories?"+snapchatPaging(params), nil)
	case "profile.spotlights", "snapchat.profile.spotlights":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/spotlights?"+snapchatPaging(params), nil)
	case "profile.saved_stories", "snapchat.profile.saved_stories":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/saved_stories?"+snapchatPaging(params), nil)
	case "spotlight.get", "snapchat.spotlight.get":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		spotlightID, err := requiredStringParam(params, "spotlightId")
		if err != nil {
			return nil, err
		}
		return s.getBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/spotlights/"+url.PathEscape(spotlightID), nil)
	// --- Public Profile API writes (JSON; media BYTES uploaded out of band) ---
	case "profile.media.create", "snapchat.profile.media.create":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for snapchat.profile.media.create")
		}
		return s.postBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/media", body, nil)
	case "story.post", "snapchat.story.post":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for snapchat.story.post")
		}
		return s.postBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/stories", body, nil)
	case "spotlight.post", "snapchat.spotlight.post":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for snapchat.spotlight.post")
		}
		return s.postBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/spotlights", body, nil)
	case "saved_story.create", "snapchat.saved_story.create":
		profileID, err := requiredStringParam(params, "profileId")
		if err != nil {
			return nil, err
		}
		if len(body) == 0 {
			return nil, fmt.Errorf("body is required for snapchat.saved_story.create")
		}
		return s.postBearer(ctx, token, profileBase+"/public_profiles/"+url.PathEscape(profileID)+"/saved_stories", body, nil)
	default:
		return nil, fmt.Errorf("unsupported Snapchat operation %q", operation)
	}
}

// snapchatPaging builds the common limit/cursor query for Public Profile list
// reads. limit is clamped to Snapchat's paging contract (default 10).
func snapchatPaging(params map[string]any) string {
	values := url.Values{"limit": {limitParam(params, "limit", 10, 100)}}
	if cursor := stringParam(params, "cursor", ""); cursor != "" {
		values.Set("cursor", cursor)
	}
	return values.Encode()
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

func (s *Service) patchBearer(ctx context.Context, token, endpoint string, body map[string]any, headers map[string]string) (any, error) {
	payload, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("marshal provider request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPatch, endpoint, bytes.NewReader(payload))
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

func (s *Service) postBearerForm(ctx context.Context, token, endpoint string, values url.Values) (any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, strings.NewReader(values.Encode()))
	if err != nil {
		return nil, fmt.Errorf("build provider request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	return s.doJSON(req)
}

func (s *Service) getBearerMap(ctx context.Context, token, endpoint string, headers map[string]string) (map[string]any, error) {
	result, err := s.getBearer(ctx, token, endpoint, headers)
	if err != nil {
		return nil, err
	}
	object, ok := result.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("provider action endpoint returned %T, want object", result)
	}
	return object, nil
}

func (s *Service) metaPageAccessToken(ctx context.Context, graphBase, userToken, pageID string) (string, error) {
	page, err := s.getBearerMap(ctx, userToken, graphBase+"/"+url.PathEscape(pageID)+"?fields=access_token,name", nil)
	if err != nil {
		return "", err
	}
	token := stringFromAny(page["access_token"])
	if token == "" {
		return "", fmt.Errorf("Meta Page %s did not return an access token", pageID)
	}
	return token, nil
}

// metaPageForInstagramAccount resolves the Facebook Page linked to an Instagram
// business account, returning the page id and page access token. Pages are
// listed via /me/accounts with the instagram_business_account field expanded;
// the match is on the IG business-account id (the id inbound IG webhooks carry
// as entry.id).
func (s *Service) metaPageForInstagramAccount(ctx context.Context, graphBase, userToken, igAccountID string) (string, string, error) {
	resp, err := s.getBearerMap(ctx, userToken, graphBase+"/me/accounts?"+url.Values{
		"fields": {"id,access_token,instagram_business_account{id}"},
		"limit":  {"100"},
	}.Encode(), nil)
	if err != nil {
		return "", "", err
	}
	pages, _ := resp["data"].([]any)
	for _, raw := range pages {
		page, ok := raw.(map[string]any)
		if !ok {
			continue
		}
		ig, _ := page["instagram_business_account"].(map[string]any)
		if ig == nil || stringFromAny(ig["id"]) != igAccountID {
			continue
		}
		pageID := stringFromAny(page["id"])
		pageToken := stringFromAny(page["access_token"])
		if pageID == "" || pageToken == "" {
			return "", "", fmt.Errorf("Meta Page linked to Instagram account %s did not return an id/access token", igAccountID)
		}
		return pageID, pageToken, nil
	}
	return "", "", fmt.Errorf("no Meta Page linked to Instagram business account %s is reachable with this token", igAccountID)
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

func (s *Service) linkedinHeaders() map[string]string {
	version := strings.TrimSpace(s.cfg.LinkedInMarketingVersion)
	if version == "" {
		version = "202606"
	}
	return map[string]string{
		"LinkedIn-Version":          version,
		"X-Restli-Protocol-Version": "2.0.0",
	}
}

func copyHeaders(headers map[string]string) map[string]string {
	out := make(map[string]string, len(headers))
	for key, value := range headers {
		out[key] = value
	}
	return out
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
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read provider action response: %w", err)
	}
	if len(strings.TrimSpace(string(data))) == 0 {
		if resp.StatusCode < 200 || resp.StatusCode >= 300 {
			return nil, fmt.Errorf("provider action endpoint returned status %d", resp.StatusCode)
		}
		result := map[string]any{"ok": true}
		if restliID := strings.TrimSpace(resp.Header.Get("x-restli-id")); restliID != "" {
			result["id"] = restliID
		}
		return result, nil
	}
	var decoded any
	if err := json.Unmarshal(data, &decoded); err != nil {
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

func requiredStringParam(params map[string]any, key string) (string, error) {
	value := stringParam(params, key, "")
	if value == "" {
		return "", fmt.Errorf("%s is required", key)
	}
	return value, nil
}

// whatsAppMessagePayload validates a caller-supplied WhatsApp Cloud API
// message body and defaults messaging_product, which Meta requires on every
// send and does not default itself — a caller that omits it gets a Graph
// API 400 with no indication why. `to` and `type` cannot be defaulted
// (there's no sane guess for either) so they're required as given.
func whatsAppMessagePayload(body map[string]any) (map[string]any, error) {
	if _, ok := body["to"]; !ok {
		return nil, fmt.Errorf("to is required for whatsapp.messages.send")
	}
	if _, ok := body["type"]; !ok {
		return nil, fmt.Errorf("type is required for whatsapp.messages.send")
	}
	message := make(map[string]any, len(body)+1)
	for key, value := range body {
		message[key] = value
	}
	if _, ok := message["messaging_product"]; !ok {
		message["messaging_product"] = "whatsapp"
	}
	return message, nil
}

func stringSliceParam(params map[string]any, key string) []string {
	value, ok := params[key]
	if !ok {
		return nil
	}
	switch typed := value.(type) {
	case []string:
		out := make([]string, 0, len(typed))
		for _, item := range typed {
			if trimmed := strings.TrimSpace(item); trimmed != "" {
				out = append(out, trimmed)
			}
		}
		return out
	case []any:
		out := make([]string, 0, len(typed))
		for _, item := range typed {
			if text := stringFromAny(item); text != "" {
				out = append(out, text)
			}
		}
		return out
	case string:
		if strings.Contains(typed, ",") {
			parts := strings.Split(typed, ",")
			out := make([]string, 0, len(parts))
			for _, part := range parts {
				if trimmed := strings.TrimSpace(part); trimmed != "" {
					out = append(out, trimmed)
				}
			}
			return out
		}
		if trimmed := strings.TrimSpace(typed); trimmed != "" {
			return []string{trimmed}
		}
	}
	return nil
}

func stringFromAny(value any) string {
	switch typed := value.(type) {
	case string:
		return strings.TrimSpace(typed)
	case fmt.Stringer:
		return strings.TrimSpace(typed.String())
	case float64:
		return fmt.Sprintf("%.0f", typed)
	case int:
		return fmt.Sprintf("%d", typed)
	case int64:
		return fmt.Sprintf("%d", typed)
	case json.Number:
		return typed.String()
	default:
		return ""
	}
}

func valuesFromMap(input map[string]any) url.Values {
	values := url.Values{}
	for key, value := range input {
		key = strings.TrimSpace(key)
		if key == "" || value == nil {
			continue
		}
		switch typed := value.(type) {
		case string:
			if trimmed := strings.TrimSpace(typed); trimmed != "" {
				values.Set(key, trimmed)
			}
		case []string:
			for _, item := range typed {
				if trimmed := strings.TrimSpace(item); trimmed != "" {
					values.Add(key, trimmed)
				}
			}
		case bool:
			values.Set(key, fmt.Sprintf("%t", typed))
		case int:
			values.Set(key, fmt.Sprintf("%d", typed))
		case int64:
			values.Set(key, fmt.Sprintf("%d", typed))
		case float64:
			values.Set(key, fmt.Sprintf("%v", typed))
		default:
			data, err := json.Marshal(typed)
			if err == nil {
				values.Set(key, string(data))
			}
		}
	}
	return values
}

func adAccountPath(params map[string]any) (string, error) {
	id := firstNonEmpty(stringParam(params, "adAccountId", ""), stringParam(params, "accountId", ""))
	if id == "" {
		return "", fmt.Errorf("adAccountId is required")
	}
	id = strings.TrimPrefix(strings.TrimSpace(id), "act_")
	return "act_" + url.PathEscape(id), nil
}

func githubRepoPath(params map[string]any) (string, error) {
	owner := stringParam(params, "owner", "")
	repo := stringParam(params, "repo", "")
	if owner == "" || repo == "" {
		return "", fmt.Errorf("owner and repo are required")
	}
	return "/repos/" + url.PathEscape(owner) + "/" + url.PathEscape(repo), nil
}

func githubContentPath(params map[string]any) (string, error) {
	rawPath := stringParam(params, "path", "")
	if rawPath == "" {
		return "", fmt.Errorf("path is required")
	}
	segments := strings.Split(strings.Trim(rawPath, "/"), "/")
	escaped := make([]string, 0, len(segments))
	for _, segment := range segments {
		segment = strings.TrimSpace(segment)
		if segment == "" || segment == "." || segment == ".." {
			return "", fmt.Errorf("path contains an invalid segment")
		}
		escaped = append(escaped, url.PathEscape(segment))
	}
	return strings.Join(escaped, "/"), nil
}

func metaOEmbedEndpoint(kind string) (string, error) {
	switch strings.TrimSpace(strings.ToLower(kind)) {
	case "", "post", "facebook.post":
		return "oembed_post", nil
	case "video", "facebook.video", "live", "live_video":
		return "oembed_video", nil
	case "page", "facebook.page":
		return "oembed_page", nil
	case "instagram", "instagram.post", "instagram.reel":
		return "instagram_oembed", nil
	case "threads", "threads.post":
		return "threads_oembed", nil
	default:
		return "", fmt.Errorf("unsupported oEmbed kind %q", kind)
	}
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
