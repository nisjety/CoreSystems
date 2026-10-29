package planeclient

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
)

const responseLimit = 256 << 10

func (c *Client) post(ctx context.Context, plane, origin, path, bearer, inference string, body any, result any) error {
	raw, err := json.Marshal(body)
	if err != nil {
		return ErrInvalidInput
	}
	request, err := http.NewRequestWithContext(ctx, "POST", strings.TrimRight(origin, "/")+path, bytes.NewReader(raw))
	if err != nil {
		return unavailable(plane, "source_unavailable")
	}
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+bearer)
	if inference != "" {
		request.Header.Set("X-Inference-Authorization", "Bearer "+inference)
	}
	response, err := c.http.Do(request)
	if err != nil {
		return unavailable(plane, "source_unavailable")
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		reason := "source_unavailable"
		if response.StatusCode == 401 || response.StatusCode == 403 {
			reason = "access_denied"
		}
		if response.StatusCode == 429 {
			reason = "quota_limited"
		}
		return unavailable(plane, reason)
	}
	if !strings.HasPrefix(response.Header.Get("Content-Type"), "application/json") {
		return unavailable(plane, "invalid_source")
	}
	raw, err = io.ReadAll(io.LimitReader(response.Body, responseLimit+1))
	if err != nil || len(raw) > responseLimit {
		return unavailable(plane, "invalid_source")
	}
	if json.Unmarshal(raw, result) != nil {
		return unavailable(plane, "invalid_source")
	}
	return nil
}
