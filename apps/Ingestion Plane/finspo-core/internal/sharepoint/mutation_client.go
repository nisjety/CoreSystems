package sharepoint

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// MutationClient performs destructive / state-changing Graph operations on
// DriveItems: delete (to recycle bin) and move (for archive). It is used only
// by the executor when acting on a human-approved governance proposal.
//
// Write scopes required on the connection: Files.ReadWrite.All (or
// Sites.ReadWrite.All). The default read-only Nango seed does NOT grant these
// — a 403 here means the connection needs re-consent with write scopes.
type MutationClient struct {
	baseURL       string
	httpClient    *http.Client
	tokenProvider AccessTokenProvider
}

type MutationClientConfig struct {
	BaseURL       string
	HTTPClient    *http.Client
	TokenProvider AccessTokenProvider
}

func NewMutationClient(cfg MutationClientConfig) *MutationClient {
	baseURL := strings.TrimSpace(cfg.BaseURL)
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	return &MutationClient{
		baseURL:       strings.TrimRight(baseURL, "/"),
		httpClient:    httpClient,
		tokenProvider: cfg.TokenProvider,
	}
}

// DeleteItem sends DELETE /v1.0/drives/{drive}/items/{item}. Graph moves the
// item to the site recycle bin (recoverable) rather than purging it — which
// is exactly what we want for a reviewed governance action.
func (c *MutationClient) DeleteItem(ctx context.Context, organizationID, driveID, itemID string) error {
	token, err := c.token(ctx, organizationID)
	if err != nil {
		return err
	}
	url := c.baseURL + "/v1.0/drives/" + driveID + "/items/" + itemID
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, url, nil)
	if err != nil {
		return fmt.Errorf("create delete request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("delete request: %w", err)
	}
	defer resp.Body.Close()

	// Graph returns 204 No Content on a successful delete.
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return readGraphError("delete item", resp)
	}
	return nil
}

// MoveItem sends PATCH /v1.0/drives/{drive}/items/{item} with a new
// parentReference, relocating the item to destFolderID within the SAME drive.
// Cross-drive moves are not supported by a single PATCH and are rejected by
// the executor before reaching here.
func (c *MutationClient) MoveItem(ctx context.Context, organizationID, driveID, itemID, destFolderID string) error {
	if strings.TrimSpace(destFolderID) == "" {
		return fmt.Errorf("move item: destination folder id is empty")
	}
	token, err := c.token(ctx, organizationID)
	if err != nil {
		return err
	}

	body, err := json.Marshal(map[string]any{
		"parentReference": map[string]string{"id": destFolderID},
	})
	if err != nil {
		return fmt.Errorf("marshal move body: %w", err)
	}

	url := c.baseURL + "/v1.0/drives/" + driveID + "/items/" + itemID
	req, err := http.NewRequestWithContext(ctx, http.MethodPatch, url, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("create move request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("move request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return readGraphError("move item", resp)
	}
	return nil
}

func (c *MutationClient) token(ctx context.Context, organizationID string) (string, error) {
	if c.tokenProvider == nil {
		return "", ErrNotConfigured
	}
	token, err := c.tokenProvider.AccessToken(ctx, organizationID)
	if err != nil {
		return "", fmt.Errorf("resolve access token: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return "", ErrNotConfigured
	}
	return token, nil
}
