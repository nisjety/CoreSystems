package integration

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
)

// actionPayload matches integration-corev2's actions/execute response body:
// {success: true, data: {action: {providerKey, operation, result}}}.
type actionPayload struct {
	Action struct {
		ProviderKey string          `json:"providerKey"`
		Operation   string          `json:"operation"`
		Result      json.RawMessage `json:"result"`
	} `json:"action"`
}

// ExecuteAction routes a provider operation through integration-corev2's
// centralized actions surface (POST /api/v1/actions/execute). The provider
// result is returned as raw JSON — provider payload schemas are never assumed
// here; callers decode defensively.
func (c *Client) ExecuteAction(ctx context.Context, request social.ActionRequest) (*social.ActionResult, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	connectionID := strings.TrimSpace(request.ConnectionID)
	operation := strings.TrimSpace(request.Operation)
	if connectionID == "" {
		return nil, fmt.Errorf("connection id is required")
	}
	if operation == "" {
		return nil, fmt.Errorf("operation is required")
	}

	requestBody := map[string]any{
		"connectionId": connectionID,
		"operation":    operation,
	}
	if len(request.Params) > 0 {
		requestBody["params"] = request.Params
	}
	if len(request.Body) > 0 {
		requestBody["body"] = request.Body
	}
	payload, err := json.Marshal(requestBody)
	if err != nil {
		return nil, fmt.Errorf("encode action request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/api/v1/actions/execute", bytes.NewReader(payload))
	if err != nil {
		return nil, err
	}
	c.addInternalHeaders(req)
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("read integration-core action response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("integration-core actions/execute (%s) returned status %d: %s",
			operation, resp.StatusCode, strings.TrimSpace(string(raw)))
	}

	var env envelope
	if err := json.Unmarshal(raw, &env); err != nil {
		return nil, fmt.Errorf("decode integration-core action response: %w", err)
	}
	if !env.Success {
		if env.Error != nil {
			return nil, fmt.Errorf("%s: %s", env.Error.Code, env.Error.Message)
		}
		return nil, fmt.Errorf("integration-core actions/execute (%s) failed: %s",
			operation, strings.TrimSpace(string(raw)))
	}

	var data actionPayload
	if len(env.Data) > 0 {
		if err := json.Unmarshal(env.Data, &data); err != nil {
			return nil, fmt.Errorf("decode integration-core action data: %w", err)
		}
	}
	return &social.ActionResult{
		ProviderKey: normalizeProvider(data.Action.ProviderKey),
		Operation:   firstNonEmpty(data.Action.Operation, operation),
		Result:      data.Action.Result,
	}, nil
}
