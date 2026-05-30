package browser

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/quarry/internal/session"
)

type Client struct {
	baseURL     string
	internalKey string
	httpClient  *http.Client
}

func NewClient(baseURL, internalKey string, timeout time.Duration) *Client {
	if timeout <= 0 {
		timeout = 30 * time.Second
	}
	return &Client{
		baseURL:     strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		internalKey: strings.TrimSpace(internalKey),
		httpClient:  &http.Client{Timeout: timeout},
	}
}

func (c *Client) Create(ctx context.Context, req CreateRequest) (*CreateResponse, error) {
	var response CreateResponse
	if err := c.doJSON(ctx, http.MethodPost, "/internal/browser/sessions", req, &response); err != nil {
		return nil, err
	}
	return &response, nil
}

func (c *Client) Get(ctx context.Context, id string) (*SessionState, error) {
	var response struct {
		Success bool         `json:"success"`
		State   SessionState `json:"state"`
	}
	if err := c.doJSON(ctx, http.MethodGet, "/internal/browser/sessions/"+id, nil, &response); err != nil {
		return nil, err
	}
	return &response.State, nil
}

func (c *Client) List(ctx context.Context) ([]session.SessionInfo, error) {
	var response struct {
		Success bool                  `json:"success"`
		Data    []session.SessionInfo `json:"data"`
	}
	if err := c.doJSON(ctx, http.MethodGet, "/internal/browser/sessions", nil, &response); err != nil {
		return nil, err
	}
	return response.Data, nil
}

func (c *Client) Execute(ctx context.Context, id string, req ExecuteRequest) (*ExecuteResponse, error) {
	var response ExecuteResponse
	if err := c.doJSON(ctx, http.MethodPost, "/internal/browser/sessions/"+id+"/execute", req, &response); err != nil {
		return nil, err
	}
	return &response, nil
}

func (c *Client) HTML(ctx context.Context, id string) (*HTMLResponse, error) {
	var response HTMLResponse
	if err := c.doJSON(ctx, http.MethodGet, "/internal/browser/sessions/"+id+"/html", nil, &response); err != nil {
		return nil, err
	}
	return &response, nil
}

func (c *Client) Live(ctx context.Context, id string) (*LiveResponse, error) {
	var response LiveResponse
	if err := c.doJSON(ctx, http.MethodGet, "/internal/browser/sessions/"+id+"/live", nil, &response); err != nil {
		return nil, err
	}
	return &response, nil
}

func (c *Client) Delete(ctx context.Context, id string) error {
	return c.doJSON(ctx, http.MethodDelete, "/internal/browser/sessions/"+id, nil, nil)
}

func (c *Client) Close() error {
	if c == nil || c.httpClient == nil {
		return nil
	}
	c.httpClient.CloseIdleConnections()
	return nil
}

func (c *Client) doJSON(ctx context.Context, method, path string, body interface{}, target interface{}) error {
	if c == nil || c.httpClient == nil || c.baseURL == "" {
		return fmt.Errorf("browser client is not configured")
	}

	var payload io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return err
		}
		payload = bytes.NewReader(encoded)
	}

	req, err := http.NewRequestWithContext(ctx, method, c.baseURL+path, payload)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if c.internalKey != "" {
		req.Header.Set("X-Internal-API-Key", c.internalKey)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= http.StatusBadRequest {
		data, _ := io.ReadAll(resp.Body)
		if len(data) == 0 {
			return fmt.Errorf("browser service returned status %d", resp.StatusCode)
		}
		return fmt.Errorf("browser service returned status %d: %s", resp.StatusCode, strings.TrimSpace(string(data)))
	}
	if target == nil {
		io.Copy(io.Discard, resp.Body)
		return nil
	}
	return json.NewDecoder(resp.Body).Decode(target)
}
