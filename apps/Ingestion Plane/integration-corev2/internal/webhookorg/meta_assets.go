package webhookorg

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// TokenSource resolves a fresh access token for a connection (implemented by
// oauth.Service).
type TokenSource interface {
	AccessTokenForConnection(ctx context.Context, connectionID string) (oauth.AccessTokenResult, error)
}

// GraphAssetLister sweeps a Meta connection's webhook-relevant asset ids via
// the Graph API:
//   - /me/accounts?fields=id,instagram_business_account{id} → Page ids + the
//     linked IG business-account ids (Page id = Messenger entry.id; IG id =
//     Instagram entry.id)
//   - /me/businesses?fields=owned_whatsapp_business_accounts{id} → WABA ids
//     (WhatsApp entry.id), then /{waba}/phone_numbers → phone_number_ids
//     (value.metadata.phone_number_id)
//
// Discovery and subscription are capability-gated. Page/Instagram failures
// are terminal when inbox/Messenger is enabled; WABA failures are terminal
// only when WhatsApp management is enabled.
type GraphAssetLister struct {
	BaseURL          string // e.g. https://graph.facebook.com/v25.0
	InstagramBaseURL string // e.g. https://graph.instagram.com/v25.0
	Tokens           TokenSource
	HTTP             *http.Client
}

type graphIDPage struct {
	Data []struct {
		ID string `json:"id"`
	} `json:"data"`
	Paging struct {
		Next string `json:"next"`
	} `json:"paging"`
}

type graphBusinessPage struct {
	Data []struct {
		OwnedWhatsAppBusinessAccounts graphIDPage `json:"owned_whatsapp_business_accounts"`
	} `json:"data"`
	Paging struct {
		Next string `json:"next"`
	} `json:"paging"`
}

func (g *GraphAssetLister) ListWebhookAccountIDs(ctx context.Context, conn store.Connection) ([]string, error) {
	assets, err := g.ListWebhookAssets(ctx, conn)
	return assets.AccountIDs, err
}

func (g *GraphAssetLister) ListWebhookAssets(ctx context.Context, conn store.Connection) (MetaWebhookAssets, error) {
	// Instagram API with Instagram Login issues a graph.instagram.com token for
	// the professional account itself. It cannot enumerate Facebook Pages via
	// /me/accounts, and Instagram webhook subscriptions are configured at the
	// Meta app level rather than a per-account /subscribed_apps edge.
	if conn.ProviderKey == "instagram" {
		return g.listStandaloneInstagramAssets(ctx, conn)
	}
	return g.listWebhookAssets(ctx, conn, nil)
}

// SubscribeWebhookAccounts enables only the webhook families represented by
// the connection's provider-verified capabilities. Discovery is deliberately
// separate so Resolver can reject ambiguous tenant bindings before any
// provider-side subscription mutation occurs.
func (g *GraphAssetLister) SubscribeWebhookAccounts(ctx context.Context, conn store.Connection, accountIDs []string) error {
	if conn.ProviderKey == "instagram" {
		assets, err := g.listStandaloneInstagramAssets(ctx, conn)
		if err != nil {
			return err
		}
		for _, accountID := range accountIDs {
			if !containsAssetID(assets.InstagramAccountIDs, strings.TrimSpace(accountID)) {
				return fmt.Errorf("Instagram professional account %s no longer matches this connection", accountID)
			}
		}
		return nil
	}
	allowed := make(map[string]bool, len(accountIDs))
	for _, id := range accountIDs {
		allowed[strings.TrimSpace(id)] = true
	}
	_, err := g.listWebhookAssets(ctx, conn, allowed)
	return err
}

func (g *GraphAssetLister) listStandaloneInstagramAssets(ctx context.Context, conn store.Connection) (MetaWebhookAssets, error) {
	if !connectionHasAnyCapability(conn, "social.inbox.read") {
		return MetaWebhookAssets{}, nil
	}
	if g.Tokens == nil {
		return MetaWebhookAssets{}, fmt.Errorf("graph asset lister has no token source")
	}
	token, err := g.Tokens.AccessTokenForConnection(ctx, conn.ID)
	if err != nil {
		return MetaWebhookAssets{}, fmt.Errorf("resolve Instagram token: %w", err)
	}
	var profile struct {
		ID string `json:"id"`
	}
	baseURL := g.instagramBaseURL()
	if err := g.getJSONFromBase(ctx, token.AccessToken, baseURL, baseURL+"/me?fields=id,username", &profile); err != nil {
		return MetaWebhookAssets{}, fmt.Errorf("read Instagram professional account: %w", err)
	}
	accountID := strings.TrimSpace(profile.ID)
	if accountID == "" {
		return MetaWebhookAssets{}, fmt.Errorf("Instagram Login returned no professional account id")
	}
	return MetaWebhookAssets{
		AccountIDs:          []string{accountID},
		InstagramAccountIDs: []string{accountID},
	}, nil
}

func (g *GraphAssetLister) listWebhookAssets(ctx context.Context, conn store.Connection, subscribeIDs map[string]bool) (MetaWebhookAssets, error) {
	if g.Tokens == nil {
		return MetaWebhookAssets{}, fmt.Errorf("graph asset lister has no token source")
	}
	token, err := g.Tokens.AccessTokenForConnection(ctx, conn.ID)
	if err != nil {
		return MetaWebhookAssets{}, fmt.Errorf("resolve token: %w", err)
	}

	assets := MetaWebhookAssets{}
	foundWABA := false
	seen := map[string]bool{}
	push := func(id string, category *[]string) {
		id = strings.TrimSpace(id)
		if id != "" && !seen[id] {
			seen[id] = true
			assets.AccountIDs = append(assets.AccountIDs, id)
		}
		if id != "" && !containsAssetID(*category, id) {
			*category = append(*category, id)
		}
	}
	wantsUnifiedInbox := connectionHasAnyCapability(conn, "social.inbox.read") && conn.ProviderKey != "whatsapp"
	wantsMessenger := connectionHasAnyCapability(conn, "social.messenger.manage")
	wantsPageInbox := wantsUnifiedInbox || wantsMessenger
	wantsWhatsApp := connectionHasAnyCapability(conn, "social.whatsapp.manage") ||
		(conn.ProviderKey == "whatsapp" && connectionHasAnyCapability(conn, "social.inbox.read"))
	requiresWhatsApp := conn.ProviderKey == "whatsapp"
	if !wantsPageInbox && !wantsWhatsApp {
		return MetaWebhookAssets{}, nil
	}

	// Pages + linked Instagram business accounts (paginated).
	pagesURL := ""
	if wantsPageInbox {
		pagesURL = g.baseURL() + "/me/accounts?" + url.Values{
			"fields": {"id,access_token,instagram_business_account{id}"},
			"limit":  {"100"},
		}.Encode()
	}
	for pageNumber := 0; pagesURL != "" && pageNumber < 10; pageNumber++ {
		var page struct {
			Data []struct {
				ID                       string `json:"id"`
				AccessToken              string `json:"access_token"`
				InstagramBusinessAccount *struct {
					ID string `json:"id"`
				} `json:"instagram_business_account"`
			} `json:"data"`
			Paging struct {
				Next string `json:"next"`
			} `json:"paging"`
		}
		if err := g.getJSON(ctx, token.AccessToken, pagesURL, &page); err != nil {
			if wantsPageInbox {
				return MetaWebhookAssets{}, fmt.Errorf("list pages: %w", err)
			}
			break
		}
		for _, item := range page.Data {
			if wantsPageInbox {
				push(item.ID, &assets.PageIDs)
			}
			if wantsUnifiedInbox && item.InstagramBusinessAccount != nil {
				push(item.InstagramBusinessAccount.ID, &assets.InstagramAccountIDs)
			}
			if subscribeIDs[item.ID] && wantsPageInbox && item.ID != "" {
				if strings.TrimSpace(item.AccessToken) == "" {
					return MetaWebhookAssets{}, fmt.Errorf("subscribe Page %s: Graph did not return a Page access token", item.ID)
				}
				if err := g.subscribe(ctx, item.AccessToken, item.ID, pageSubscriptionFields(wantsUnifiedInbox, wantsMessenger)); err != nil {
					return MetaWebhookAssets{}, fmt.Errorf("subscribe Page %s: %w", item.ID, err)
				}
			}
		}
		pagesURL, err = g.safePageURL(page.Paging.Next)
		if err != nil {
			return MetaWebhookAssets{}, fmt.Errorf("list pages pagination: %w", err)
		}
	}

	// WhatsApp Business Accounts + their phone numbers (all result levels are
	// paginated; agencies commonly exceed the first 50 assets).
	businessesURL := ""
	if wantsWhatsApp {
		businessesURL = g.baseURL() + "/me/businesses?" + url.Values{
			"fields": {"owned_whatsapp_business_accounts{id}"},
			"limit":  {"50"},
		}.Encode()
	}
	if businessesURL == "" {
		return assets, nil
	}
	processedWABAs := map[string]bool{}
	processWABAPage := func(page graphIDPage) error {
		wabaPage := page
		for pageNumber := 0; pageNumber < 10; pageNumber++ {
			for _, waba := range wabaPage.Data {
				if processedWABAs[waba.ID] {
					continue
				}
				processedWABAs[waba.ID] = true
				foundWABA = true
				push(waba.ID, &assets.WhatsAppBusinessAccountIDs)
				if subscribeIDs[waba.ID] {
					if err := g.subscribe(ctx, token.AccessToken, waba.ID, ""); err != nil {
						return fmt.Errorf("subscribe WhatsApp Business Account %s: %w", waba.ID, err)
					}
				}
				numbersURL := g.baseURL() + "/" + url.PathEscape(waba.ID) + "/phone_numbers?fields=id&limit=50"
				for numberPage := 0; numbersURL != "" && numberPage < 10; numberPage++ {
					var numbers graphIDPage
					if err := g.getJSON(ctx, token.AccessToken, numbersURL, &numbers); err != nil {
						return fmt.Errorf("list phone numbers for WhatsApp Business Account %s: %w", waba.ID, err)
					}
					for _, number := range numbers.Data {
						push(number.ID, &assets.WhatsAppPhoneNumberIDs)
					}
					var err error
					numbersURL, err = g.safePageURL(numbers.Paging.Next)
					if err != nil {
						return fmt.Errorf("list WhatsApp phone numbers pagination: %w", err)
					}
				}
			}
			if wabaPage.Paging.Next == "" {
				return nil
			}
			next, err := g.safePageURL(wabaPage.Paging.Next)
			if err != nil {
				return fmt.Errorf("list WhatsApp Business Accounts pagination: %w", err)
			}
			wabaPage = graphIDPage{}
			if err := g.getJSON(ctx, token.AccessToken, next, &wabaPage); err != nil {
				return err
			}
		}
		return fmt.Errorf("WhatsApp Business Account pagination exceeded the safety limit")
	}
	for businessPage := 0; businessesURL != "" && businessPage < 10; businessPage++ {
		var businesses graphBusinessPage
		if err := g.getJSON(ctx, token.AccessToken, businessesURL, &businesses); err != nil {
			return MetaWebhookAssets{}, fmt.Errorf("list WhatsApp Business Accounts: %w", err)
		}
		for _, business := range businesses.Data {
			if err := processWABAPage(business.OwnedWhatsAppBusinessAccounts); err != nil {
				return MetaWebhookAssets{}, err
			}
		}
		var err error
		businessesURL, err = g.safePageURL(businesses.Paging.Next)
		if err != nil {
			return MetaWebhookAssets{}, fmt.Errorf("list Meta businesses pagination: %w", err)
		}
	}
	if requiresWhatsApp && !foundWABA {
		return MetaWebhookAssets{}, fmt.Errorf("Meta connection has no accessible WhatsApp Business Account")
	}

	return assets, nil
}

func containsAssetID(ids []string, id string) bool {
	for _, value := range ids {
		if value == id {
			return true
		}
	}
	return false
}

func pageSubscriptionFields(wantsUnifiedInbox, wantsMessenger bool) string {
	fields := []string{}
	if wantsUnifiedInbox {
		fields = append(fields, "feed", "messages")
	}
	if wantsMessenger {
		fields = append(fields, "messages", "messaging_postbacks", "messaging_optins", "message_deliveries", "message_reads")
	}
	seen := map[string]struct{}{}
	deduped := make([]string, 0, len(fields))
	for _, field := range fields {
		if _, ok := seen[field]; ok {
			continue
		}
		seen[field] = struct{}{}
		deduped = append(deduped, field)
	}
	return strings.Join(deduped, ",")
}

func connectionHasAnyCapability(conn store.Connection, capabilities ...string) bool {
	wanted := make(map[string]struct{}, len(capabilities))
	for _, capability := range capabilities {
		wanted[capability] = struct{}{}
	}
	for _, capability := range conn.Capabilities {
		if _, ok := wanted[capability]; ok {
			return true
		}
	}
	return false
}

func (g *GraphAssetLister) safePageURL(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", nil
	}
	base, err := url.Parse(g.baseURL())
	if err != nil {
		return "", fmt.Errorf("parse Graph base URL: %w", err)
	}
	next, err := url.Parse(raw)
	if err != nil || !next.IsAbs() || next.Scheme != base.Scheme || next.Host != base.Host || next.User != nil {
		return "", fmt.Errorf("provider returned an off-origin pagination URL")
	}
	next.Fragment = ""
	return next.String(), nil
}

func (g *GraphAssetLister) subscribe(ctx context.Context, accessToken, accountID, fields string) error {
	values := url.Values{}
	if strings.TrimSpace(fields) != "" {
		values.Set("subscribed_fields", fields)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, g.baseURL()+"/"+url.PathEscape(accountID)+"/subscribed_apps", strings.NewReader(values.Encode()))
	if err != nil {
		return fmt.Errorf("build subscription request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	client := g.HTTP
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	resp, err := sameOriginClient(client, g.baseURL()).Do(req)
	if err != nil {
		return fmt.Errorf("request subscription: %w", err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return fmt.Errorf("read subscription response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return fmt.Errorf("Graph returned %d: %s", resp.StatusCode, strings.TrimSpace(string(body[:min(len(body), 300)])))
	}
	var result struct {
		Success bool `json:"success"`
	}
	if err := json.Unmarshal(body, &result); err != nil || !result.Success {
		return fmt.Errorf("Graph did not confirm subscription success")
	}
	return nil
}

func (g *GraphAssetLister) baseURL() string {
	if strings.TrimSpace(g.BaseURL) == "" {
		return "https://graph.facebook.com/v25.0"
	}
	return strings.TrimRight(g.BaseURL, "/")
}

func (g *GraphAssetLister) instagramBaseURL() string {
	if strings.TrimSpace(g.InstagramBaseURL) == "" {
		return "https://graph.instagram.com/v25.0"
	}
	return strings.TrimRight(g.InstagramBaseURL, "/")
}

func (g *GraphAssetLister) getJSON(ctx context.Context, accessToken, fullURL string, out any) error {
	return g.getJSONFromBase(ctx, accessToken, g.baseURL(), fullURL, out)
}

func (g *GraphAssetLister) getJSONFromBase(ctx context.Context, accessToken, baseURL, fullURL string, out any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, fullURL, nil)
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	client := g.HTTP
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	resp, err := sameOriginClient(client, baseURL).Do(req)
	if err != nil {
		return fmt.Errorf("request graph: %w", err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return fmt.Errorf("read graph response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return fmt.Errorf("graph returned %d: %s", resp.StatusCode, strings.TrimSpace(string(body[:min(len(body), 300)])))
	}
	if err := json.Unmarshal(body, out); err != nil {
		return fmt.Errorf("decode graph response: %w", err)
	}
	return nil
}

func sameOriginClient(client *http.Client, baseURL string) *http.Client {
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	cloned := *client
	configured, _ := url.Parse(baseURL)
	previous := client.CheckRedirect
	cloned.CheckRedirect = func(req *http.Request, via []*http.Request) error {
		if len(via) >= 10 || configured == nil || req.URL.Scheme != configured.Scheme || req.URL.Host != configured.Host {
			return fmt.Errorf("provider redirect left the configured origin")
		}
		if previous != nil {
			return previous(req, via)
		}
		return nil
	}
	return &cloned
}
