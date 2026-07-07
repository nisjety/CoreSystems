package discovery

import (
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

type Service struct {
	cfg        config.Config
	httpClient *http.Client
}

type Snapshot struct {
	ConnectionID     string          `json:"connectionId"`
	ProviderKey      string          `json:"providerKey"`
	ConnectorType    string          `json:"connectorType"`
	WorkspaceName    string          `json:"workspaceName,omitempty"`
	WorkspaceID      string          `json:"workspaceId,omitempty"`
	AccountName      string          `json:"accountName,omitempty"`
	EntityCounts     map[string]int  `json:"entityCounts"`
	SampleEntities   []SampleEntity  `json:"sampleEntities"`
	Availability     map[string]bool `json:"availability"`
	Scopes           []string        `json:"scopes"`
	Sensitivity      string          `json:"sensitivity"`
	FetchedAt        time.Time       `json:"fetchedAt"`
	ProviderWarnings []string        `json:"providerWarnings,omitempty"`
}

type SampleEntity struct {
	Kind  string `json:"kind"`
	Label string `json:"label"`
}

func NewService(cfg config.Config, httpClient *http.Client) *Service {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 8 * time.Second}
	}
	return &Service{cfg: cfg, httpClient: httpClient}
}

func (s *Service) Discover(ctx context.Context, connection store.Connection, accessToken string) (Snapshot, error) {
	snapshot := Snapshot{
		ConnectionID:   connection.ID,
		ProviderKey:    connection.ProviderKey,
		ConnectorType:  connection.ConnectorType,
		EntityCounts:   map[string]int{},
		SampleEntities: []SampleEntity{},
		Availability:   availabilityFromScopes(connection.Scopes),
		Scopes:         connection.Scopes,
		Sensitivity:    "safe_metadata_only",
		FetchedAt:      time.Now().UTC(),
	}
	switch connection.ProviderKey {
	case "microsoft":
		return s.discoverMicrosoft(ctx, snapshot, accessToken)
	case "slack":
		return s.discoverSlack(ctx, snapshot, accessToken)
	case "google":
		return s.discoverGoogle(ctx, snapshot, accessToken)
	case "notion":
		return s.discoverNotion(ctx, snapshot, accessToken)
	case "github":
		return s.discoverGitHub(ctx, snapshot, accessToken)
	case "linkedin":
		return s.discoverLinkedIn(ctx, snapshot, accessToken)
	case "shopify":
		return s.discoverShopify(ctx, snapshot, accessToken, connection.ProviderContext)
	case "stripe":
		return s.discoverStripe(ctx, snapshot, accessToken)
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		return s.discoverMeta(ctx, snapshot, accessToken, connection.ProviderKey)
	default:
		// Identity-based providers (social: x, discord, meta family, linkedin,
		// tiktok, snapchat, …) expose no browsable source tree to enumerate —
		// the connected account IS the source. Returning an error here used to
		// 502 the onboarding discover step for perfectly healthy connections;
		// answer with an honest identity snapshot instead.
		return identitySnapshot(snapshot, connection), nil
	}
}

// identitySnapshot describes a connection whose provider has no
// tree-of-sources to enumerate: the account identity is the single source.
func identitySnapshot(snapshot Snapshot, connection store.Connection) Snapshot {
	label := strings.TrimSpace(connection.DisplayName)
	if label == "" {
		label = connection.ProviderKey
	}
	snapshot.AccountName = safeSampleLabel(label)
	snapshot.EntityCounts["accounts"] = 1
	snapshot.SampleEntities = append(snapshot.SampleEntities, SampleEntity{
		Kind:  "account",
		Label: safeSampleLabel(label),
	})
	snapshot.ProviderWarnings = append(snapshot.ProviderWarnings,
		fmt.Sprintf("%s has no browsable source tree; the connected account identity is the source.", connection.ProviderKey))
	return snapshot
}

func (s *Service) discoverMicrosoft(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.MicrosoftGraphBaseURL, "/")
	me, err := s.getJSON(ctx, accessToken, base+"/v1.0/me?$select=id,displayName,mail,userPrincipalName", nil)
	if err != nil {
		return snapshot, err
	}
	snapshot.AccountName = safeSampleLabel(stringValue(me["displayName"]))
	snapshot.WorkspaceID = stringValue(me["id"])

	if teams, err := s.getJSON(ctx, accessToken, base+"/v1.0/me/joinedTeams?$top=3", nil); err == nil {
		values := objectList(teams["value"])
		snapshot.EntityCounts["teams_sampled"] = len(values)
		snapshot.Availability["teams"] = true
	}
	if drive, err := s.getJSON(ctx, accessToken, base+"/v1.0/me/drive?$select=id,driveType,quota", nil); err == nil {
		if quota, ok := drive["quota"].(map[string]any); ok {
			if used := intValue(quota["used"]); used > 0 {
				snapshot.EntityCounts["drive_storage_used_bytes"] = used
			}
		}
		snapshot.Availability["drive"] = true
	}
	return snapshot, nil
}

func (s *Service) discoverSlack(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.SlackAPIBaseURL, "/")
	authTest, err := s.getJSON(ctx, accessToken, base+"/auth.test", nil)
	if err != nil {
		return snapshot, err
	}
	if ok, _ := authTest["ok"].(bool); authTest["ok"] != nil && !ok {
		return snapshot, fmt.Errorf("Slack auth.test failed: %s", stringValue(authTest["error"]))
	}
	snapshot.AccountName = stringValue(authTest["user"])
	snapshot.WorkspaceName = stringValue(authTest["team"])
	snapshot.WorkspaceID = stringValue(authTest["team_id"])

	channels, err := s.getJSON(ctx, accessToken, base+"/conversations.list?limit=3&types=public_channel&exclude_archived=true", nil)
	if err == nil {
		values := objectList(channels["channels"])
		snapshot.EntityCounts["public_channels_sampled"] = len(values)
		for _, channel := range values {
			if name := stringValue(channel["name"]); name != "" {
				appendSampleEntity(&snapshot, "public_channel", "#"+name)
			}
		}
		snapshot.Availability["public_channels"] = true
	}
	return snapshot, nil
}

func (s *Service) discoverGoogle(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.GoogleAPIBaseURL, "/")
	user, err := s.getJSON(ctx, accessToken, base+"/oauth2/v3/userinfo", nil)
	if err != nil {
		return snapshot, err
	}
	snapshot.AccountName = safeSampleLabel(stringValue(user["name"]))
	snapshot.WorkspaceID = stringValue(user["sub"])

	about, err := s.getJSON(ctx, accessToken, base+"/drive/v3/about?fields=user,storageQuota", nil)
	if err == nil {
		if quota, ok := about["storageQuota"].(map[string]any); ok {
			if used := intValue(quota["usage"]); used > 0 {
				snapshot.EntityCounts["drive_storage_used_bytes"] = used
			}
		}
		snapshot.Availability["drive"] = true
	}

	filesURL := base + "/drive/v3/files?" + url.Values{
		"fields":   {"files(id,name,mimeType)"},
		"pageSize": {"3"},
		"q":        {"mimeType='application/vnd.google-apps.folder' and trashed=false"},
	}.Encode()
	files, err := s.getJSON(ctx, accessToken, filesURL, nil)
	if err == nil {
		values := objectList(files["files"])
		snapshot.EntityCounts["folders_sampled"] = len(values)
		snapshot.ProviderWarnings = append(snapshot.ProviderWarnings, "Google Drive folder names are hidden during onboarding preview.")
	}
	return snapshot, nil
}

func (s *Service) discoverNotion(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.NotionAPIBaseURL, "/")
	searchBody := strings.NewReader(`{"page_size":3}`)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, base+"/v1/search", searchBody)
	if err != nil {
		return snapshot, fmt.Errorf("build notion discovery request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Notion-Version", "2022-06-28")
	body, err := s.doJSON(req)
	if err != nil {
		return snapshot, err
	}
	results := objectList(body["results"])
	snapshot.EntityCounts["objects_sampled"] = len(results)
	for _, item := range results {
		label := notionTitle(item)
		if label != "" {
			appendSampleEntity(&snapshot, firstNonEmpty(stringValue(item["object"]), "notion_object"), label)
		}
	}
	snapshot.Availability["workspace_search"] = true
	return snapshot, nil
}

func (s *Service) discoverGitHub(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.GitHubAPIBaseURL, "/")
	headers := map[string]string{
		"Accept":               "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	}
	user, err := s.getJSON(ctx, accessToken, base+"/user", headers)
	if err != nil {
		return snapshot, err
	}
	snapshot.AccountName = firstNonEmpty(stringValue(user["name"]), stringValue(user["login"]))
	snapshot.WorkspaceID = fmt.Sprintf("%v", user["id"])
	repos, err := s.getJSON(ctx, accessToken, base+"/user/repos?per_page=3&sort=updated&type=owner", headers)
	if err == nil {
		values := objectList(repos["items"])
		snapshot.EntityCounts["repositories_sampled"] = len(values)
		for _, repo := range values {
			if name := firstNonEmpty(stringValue(repo["full_name"]), stringValue(repo["name"])); name != "" {
				appendSampleEntity(&snapshot, "repository", name)
			}
		}
		snapshot.Availability["repositories"] = true
	}
	orgs, err := s.getJSON(ctx, accessToken, base+"/user/orgs?per_page=3", headers)
	if err == nil {
		values := objectList(orgs["items"])
		snapshot.EntityCounts["organizations_sampled"] = len(values)
		for _, org := range values {
			if login := stringValue(org["login"]); login != "" {
				appendSampleEntity(&snapshot, "organization", login)
			}
		}
		snapshot.Availability["organizations"] = true
	}
	return snapshot, nil
}

func (s *Service) discoverLinkedIn(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.LinkedInAPIBaseURL, "/")
	user, err := s.getJSON(ctx, accessToken, base+"/v2/userinfo", nil)
	if err != nil {
		return snapshot, err
	}
	snapshot.AccountName = safeSampleLabel(firstNonEmpty(stringValue(user["name"]), strings.TrimSpace(stringValue(user["given_name"])+" "+stringValue(user["family_name"]))))
	snapshot.WorkspaceID = stringValue(user["sub"])
	snapshot.Availability["profile"] = true
	if stringValue(user["email"]) != "" {
		snapshot.Availability["email"] = true
	}

	headers := linkedInHeaders(s.cfg)
	orgs, err := s.getJSON(ctx, accessToken, base+"/rest/organizationAcls?"+url.Values{"q": {"roleAssignee"}, "count": {"3"}}.Encode(), headers)
	if err == nil {
		values := objectList(orgs["elements"])
		snapshot.EntityCounts["organizations_sampled"] = len(values)
		for _, org := range values {
			if organization := stringValue(org["organization"]); organization != "" {
				appendSampleEntity(&snapshot, "organization", organization)
			}
		}
		snapshot.Availability["organizations"] = true
	}
	return snapshot, nil
}

func (s *Service) discoverShopify(ctx context.Context, snapshot Snapshot, accessToken string, providerContext map[string]string) (Snapshot, error) {
	shop, err := oauth.ShopifyShop(providerContext)
	if err != nil {
		return snapshot, err
	}
	shopBody, err := s.getShopifyJSON(ctx, accessToken, "https://"+shop+"/admin/api/2026-01/shop.json")
	if err != nil {
		return snapshot, err
	}
	if shopData, ok := shopBody["shop"].(map[string]any); ok {
		snapshot.WorkspaceName = firstNonEmpty(stringValue(shopData["name"]), stringValue(shopData["myshopify_domain"]), shop)
		snapshot.WorkspaceID = firstNonEmpty(stringValue(shopData["myshopify_domain"]), shop)
	}
	if countBody, err := s.getShopifyJSON(ctx, accessToken, "https://"+shop+"/admin/api/2026-01/products/count.json"); err == nil {
		snapshot.EntityCounts["products"] = intValue(countBody["count"])
		snapshot.Availability["products"] = true
	}
	return snapshot, nil
}

func (s *Service) discoverStripe(ctx context.Context, snapshot Snapshot, accessToken string) (Snapshot, error) {
	base := strings.TrimRight(s.cfg.StripeAPIBaseURL, "/")
	account, err := s.getJSON(ctx, accessToken, base+"/v1/account", nil)
	if err != nil {
		return snapshot, err
	}
	businessProfile, _ := account["business_profile"].(map[string]any)
	snapshot.WorkspaceID = stringValue(account["id"])
	snapshot.WorkspaceName = firstNonEmpty(stringValue(account["business_name"]), stringValue(account["display_name"]), stringValue(businessProfile["name"]), stringValue(account["id"]))
	snapshot.AccountName = snapshot.WorkspaceName
	snapshot.Availability["customers"] = true
	snapshot.Availability["subscriptions"] = true
	snapshot.Availability["invoices"] = true
	snapshot.EntityCounts["billing_areas"] = 3
	return snapshot, nil
}

func (s *Service) discoverMeta(ctx context.Context, snapshot Snapshot, accessToken, providerKey string) (Snapshot, error) {
	base := strings.TrimRight(firstNonEmpty(s.cfg.FacebookAPIBaseURL, s.cfg.InstagramAPIBaseURL, "https://graph.facebook.com/v25.0"), "/")
	if providerKey == "instagram" && strings.TrimSpace(s.cfg.InstagramAPIBaseURL) != "" {
		base = strings.TrimRight(s.cfg.InstagramAPIBaseURL, "/")
	}
	me, err := s.getJSON(ctx, accessToken, base+"/me?fields=id,name", nil)
	if err != nil {
		return snapshot, err
	}
	snapshot.AccountName = safeSampleLabel(stringValue(me["name"]))
	snapshot.WorkspaceID = stringValue(me["id"])
	snapshot.Availability["profile"] = true

	pagesURL := base + "/me/accounts?" + url.Values{
		"fields": {"id,name,category,instagram_business_account{id,username}"},
		"limit":  {"3"},
	}.Encode()
	if pages, err := s.getJSON(ctx, accessToken, pagesURL, nil); err == nil {
		values := objectList(pages["data"])
		snapshot.EntityCounts["pages_sampled"] = len(values)
		for _, page := range values {
			if name := stringValue(page["name"]); name != "" {
				appendSampleEntity(&snapshot, "facebook_page", safeSampleLabel(name))
			}
			if ig, ok := page["instagram_business_account"].(map[string]any); ok {
				if username := stringValue(ig["username"]); username != "" {
					appendSampleEntity(&snapshot, "instagram_account", safeSampleLabel("@"+username))
				}
			}
		}
		snapshot.Availability["pages"] = true
	}

	if adAccounts, err := s.getJSON(ctx, accessToken, base+"/me/adaccounts?fields=id,name,account_status&limit=3", nil); err == nil {
		values := objectList(adAccounts["data"])
		snapshot.EntityCounts["ad_accounts_sampled"] = len(values)
		for _, account := range values {
			if name := stringValue(account["name"]); name != "" {
				appendSampleEntity(&snapshot, "ad_account", safeSampleLabel(name))
			}
		}
		snapshot.Availability["ad_accounts"] = true
	}

	businessesURL := base + "/me/businesses?" + url.Values{
		"fields": {"id,name,owned_whatsapp_business_accounts{id,name}"},
		"limit":  {"3"},
	}.Encode()
	if businesses, err := s.getJSON(ctx, accessToken, businessesURL, nil); err == nil {
		values := objectList(businesses["data"])
		snapshot.EntityCounts["businesses_sampled"] = len(values)
		for _, business := range values {
			if name := stringValue(business["name"]); name != "" {
				appendSampleEntity(&snapshot, "business", safeSampleLabel(name))
			}
			if wabas, ok := business["owned_whatsapp_business_accounts"].(map[string]any); ok {
				accounts := objectList(wabas["data"])
				snapshot.EntityCounts["whatsapp_business_accounts_sampled"] += len(accounts)
				for _, account := range accounts {
					if name := stringValue(account["name"]); name != "" {
						appendSampleEntity(&snapshot, "whatsapp_business_account", safeSampleLabel(name))
					}
				}
			}
		}
		snapshot.Availability["businesses"] = true
	}

	return snapshot, nil
}

func (s *Service) getJSON(ctx context.Context, accessToken, endpoint string, headers map[string]string) (map[string]any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("build discovery request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	return s.doJSON(req)
}

func (s *Service) getShopifyJSON(ctx context.Context, accessToken, endpoint string) (map[string]any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("build shopify discovery request: %w", err)
	}
	req.Header.Set("X-Shopify-Access-Token", accessToken)
	req.Header.Set("Accept", "application/json")
	return s.doJSON(req)
}

func (s *Service) doJSON(req *http.Request) (map[string]any, error) {
	resp, err := s.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call discovery endpoint: %w", err)
	}
	defer resp.Body.Close()
	var body any
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		return nil, fmt.Errorf("decode discovery response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("discovery endpoint returned status %d", resp.StatusCode)
	}
	if object, ok := body.(map[string]any); ok {
		return object, nil
	}
	if list, ok := body.([]any); ok {
		return map[string]any{"items": list}, nil
	}
	return map[string]any{}, nil
}

func availabilityFromScopes(scopes []string) map[string]bool {
	availability := map[string]bool{}
	for _, scope := range scopes {
		normalized := strings.TrimSpace(strings.ToLower(scope))
		if strings.Contains(normalized, "mail") || strings.Contains(normalized, "gmail") {
			availability["mail"] = true
		}
		if strings.Contains(normalized, "drive") || strings.Contains(normalized, "files") || strings.Contains(normalized, "sites") {
			availability["documents"] = true
		}
		if strings.Contains(normalized, "team") || strings.Contains(normalized, "channel") {
			availability["collaboration"] = true
		}
		if strings.Contains(normalized, "products") {
			availability["products"] = true
		}
		if strings.Contains(normalized, "orders") {
			availability["orders"] = true
		}
		if strings.Contains(normalized, "pages_messaging") {
			availability["messenger"] = true
		}
		if strings.Contains(normalized, "pages_") {
			availability["pages"] = true
		}
		if strings.Contains(normalized, "instagram_") {
			availability["instagram"] = true
		}
		if strings.Contains(normalized, "whatsapp_") {
			availability["whatsapp"] = true
		}
		if strings.Contains(normalized, "ads_") || strings.Contains(normalized, "business_management") {
			availability["ads"] = true
		}
		if strings.Contains(normalized, "catalog_management") {
			availability["catalogs"] = true
		}
		if strings.Contains(normalized, "threads_") {
			availability["threads"] = true
		}
		if strings.Contains(normalized, "oembed") {
			availability["embeds"] = true
		}
		switch normalized {
		case "openid", "profile", "r_profile_basicinfo":
			availability["profile"] = true
		case "email":
			availability["email"] = true
		case "w_member_social":
			availability["publishing"] = true
		case "r_member_social":
			availability["posts"] = true
		case "r_organization_social", "w_organization_social", "rw_organization_admin", "r_organization_admin":
			availability["organizations"] = true
		case "r_verify", "r_verify_details":
			availability["verification"] = true
		case "r_ads", "rw_ads":
			availability["ads"] = true
		case "rw_conversions":
			availability["conversions"] = true
		case "r_marketing_leadgen_automation":
			availability["leads"] = true
		case "read:user":
			availability["profile"] = true
		case "user:email":
			availability["email"] = true
		case "read:org":
			availability["organizations"] = true
		case "public_repo":
			availability["repositories"] = true
		case "repo":
			availability["repositories"] = true
			availability["private_repositories"] = true
		}
	}
	return availability
}

func linkedInHeaders(cfg config.Config) map[string]string {
	version := strings.TrimSpace(cfg.LinkedInMarketingVersion)
	if version == "" {
		version = "202606"
	}
	return map[string]string{
		"LinkedIn-Version":          version,
		"X-Restli-Protocol-Version": "2.0.0",
	}
}

func objectList(value any) []map[string]any {
	switch typed := value.(type) {
	case []any:
		out := make([]map[string]any, 0, len(typed))
		for _, item := range typed {
			if object, ok := item.(map[string]any); ok {
				out = append(out, object)
			}
		}
		return out
	case []map[string]any:
		return typed
	default:
		return nil
	}
}

func notionTitle(item map[string]any) string {
	for _, field := range []string{"title", "properties"} {
		value, ok := item[field]
		if !ok {
			continue
		}
		if title := nestedTitle(value); title != "" {
			return title
		}
	}
	return firstNonEmpty(stringValue(item["url"]), stringValue(item["id"]))
}

func nestedTitle(value any) string {
	switch typed := value.(type) {
	case []any:
		for _, item := range typed {
			if object, ok := item.(map[string]any); ok {
				if text := stringValue(object["plain_text"]); text != "" {
					return text
				}
			}
		}
	case map[string]any:
		for _, child := range typed {
			if title := nestedTitle(child); title != "" {
				return title
			}
		}
	}
	return ""
}

func appendSampleEntity(snapshot *Snapshot, kind, label string) {
	if snapshot == nil || len(snapshot.SampleEntities) >= 3 {
		return
	}
	label = safeSampleLabel(label)
	if label == "" {
		return
	}
	snapshot.SampleEntities = append(snapshot.SampleEntities, SampleEntity{
		Kind:  strings.TrimSpace(kind),
		Label: label,
	})
}

func safeSampleLabel(label string) string {
	label = strings.Join(strings.Fields(label), " ")
	if label == "" || looksLikeEmail(label) {
		return ""
	}
	const maxLabelLength = 80
	if len(label) <= maxLabelLength {
		return label
	}
	return strings.TrimSpace(label[:maxLabelLength-1]) + "..."
}

func looksLikeEmail(value string) bool {
	value = strings.TrimSpace(value)
	if strings.Contains(value, " ") {
		return false
	}
	at := strings.Index(value, "@")
	return at > 0 && at < len(value)-1 && strings.Contains(value[at+1:], ".")
}

func stringValue(value any) string {
	if s, ok := value.(string); ok {
		return s
	}
	return ""
}

func intValue(value any) int {
	switch typed := value.(type) {
	case float64:
		return int(typed)
	case int:
		return typed
	case string:
		var out int
		_, _ = fmt.Sscanf(typed, "%d", &out)
		return out
	default:
		return 0
	}
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}
