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
// Page/IG discovery failing is terminal for the sweep; WABA discovery is
// best-effort (many Meta connections have no WhatsApp assets and some token
// types cannot read /me/businesses at all).
type GraphAssetLister struct {
	BaseURL string // e.g. https://graph.facebook.com/v25.0
	Tokens  TokenSource
	HTTP    *http.Client
}

func (g *GraphAssetLister) ListWebhookAccountIDs(ctx context.Context, conn store.Connection) ([]string, error) {
	if g.Tokens == nil {
		return nil, fmt.Errorf("graph asset lister has no token source")
	}
	token, err := g.Tokens.AccessTokenForConnection(ctx, conn.ID)
	if err != nil {
		return nil, fmt.Errorf("resolve token: %w", err)
	}

	var ids []string
	seen := map[string]bool{}
	push := func(id string) {
		id = strings.TrimSpace(id)
		if id != "" && !seen[id] {
			seen[id] = true
			ids = append(ids, id)
		}
	}

	// Pages + linked Instagram business accounts (paginated).
	pagesURL := g.baseURL() + "/me/accounts?" + url.Values{
		"fields": {"id,instagram_business_account{id}"},
		"limit":  {"100"},
	}.Encode()
	for pagesURL != "" {
		var page struct {
			Data []struct {
				ID                       string `json:"id"`
				InstagramBusinessAccount *struct {
					ID string `json:"id"`
				} `json:"instagram_business_account"`
			} `json:"data"`
			Paging struct {
				Next string `json:"next"`
			} `json:"paging"`
		}
		if err := g.getJSON(ctx, token.AccessToken, pagesURL, &page); err != nil {
			return nil, fmt.Errorf("list pages: %w", err)
		}
		for _, item := range page.Data {
			push(item.ID)
			if item.InstagramBusinessAccount != nil {
				push(item.InstagramBusinessAccount.ID)
			}
		}
		pagesURL = page.Paging.Next
	}

	// WhatsApp Business Accounts + their phone numbers (best-effort).
	var businesses struct {
		Data []struct {
			OwnedWhatsAppBusinessAccounts struct {
				Data []struct {
					ID string `json:"id"`
				} `json:"data"`
			} `json:"owned_whatsapp_business_accounts"`
		} `json:"data"`
	}
	businessesURL := g.baseURL() + "/me/businesses?" + url.Values{
		"fields": {"owned_whatsapp_business_accounts{id}"},
		"limit":  {"50"},
	}.Encode()
	if err := g.getJSON(ctx, token.AccessToken, businessesURL, &businesses); err == nil {
		for _, business := range businesses.Data {
			for _, waba := range business.OwnedWhatsAppBusinessAccounts.Data {
				push(waba.ID)
				var numbers struct {
					Data []struct {
						ID string `json:"id"`
					} `json:"data"`
				}
				numbersURL := g.baseURL() + "/" + url.PathEscape(waba.ID) + "/phone_numbers?fields=id&limit=50"
				if err := g.getJSON(ctx, token.AccessToken, numbersURL, &numbers); err == nil {
					for _, number := range numbers.Data {
						push(number.ID)
					}
				}
			}
		}
	}

	return ids, nil
}

func (g *GraphAssetLister) baseURL() string {
	if strings.TrimSpace(g.BaseURL) == "" {
		return "https://graph.facebook.com/v25.0"
	}
	return strings.TrimRight(g.BaseURL, "/")
}

func (g *GraphAssetLister) getJSON(ctx context.Context, accessToken, fullURL string, out any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, fullURL, nil)
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	client := g.HTTP
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	resp, err := client.Do(req)
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
