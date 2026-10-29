package algoliamcp

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"io"
	"mime"
	"net/http"
	"strings"
	"time"
)

const maxResponse = 2 << 20

type session struct {
	resolved     map[string]tool
	metaTools    map[string]tool
	catalog      ToolCatalogShape
	diagnostic   func(Diagnostic)
	ctx          context.Context
	unauthorized func()
	http         *http.Client
	token        string
	id           int
	maxRPC       int
	sessionID    string
	protocol     string
	valid        func() bool
}

func (s *session) rpc(ctx context.Context, method string, params any) (json.RawMessage, error) {
	if s.ctx != nil {
		ctx = s.ctx
	}
	if !s.valid() {
		return nil, ErrAccess
	}
	limit := s.maxRPC
	if limit == 0 {
		limit = 14
	}
	if s.id >= limit {
		return nil, s.failure(method, 0, nil)
	}
	s.id++
	envelope := map[string]any{"jsonrpc": "2.0", "method": method, "params": params}
	notification := method == "notifications/initialized"
	if !notification {
		envelope["id"] = s.id
	}
	raw, _ := json.Marshal(envelope)
	req, err := http.NewRequestWithContext(ctx, "POST", config.AlgoliaMCPResource, bytes.NewReader(raw))
	if err != nil {
		return nil, ErrUnavailable
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json, text/event-stream")
	req.Header.Set("Authorization", "Bearer "+s.token)
	if s.sessionID != "" {
		req.Header.Set("Mcp-Session-Id", s.sessionID)
	}
	if s.protocol != "" {
		req.Header.Set("MCP-Protocol-Version", s.protocol)
	}
	res, err := s.http.Do(req)
	if err != nil {
		return nil, s.failure(method, 0, nil)
	}
	defer res.Body.Close()
	body, err := io.ReadAll(io.LimitReader(res.Body, maxResponse+1))
	if err != nil || len(body) > maxResponse || !s.valid() {
		return nil, s.failure(method, res.StatusCode, nil)
	}
	if res.StatusCode == http.StatusUnauthorized && s.unauthorized != nil {
		s.unauthorized()
	}
	if notification {
		if res.StatusCode == 202 || res.StatusCode == 204 {
			return nil, nil
		}
		return nil, s.failure(method, res.StatusCode, nil)
	}
	if res.StatusCode != 200 {
		return nil, s.failure(method, res.StatusCode, nil)
	}
	if method == "initialize" {
		id := res.Header.Get("Mcp-Session-Id")
		if len(id) > 256 || strings.ContainsAny(id, "\r\n\x00") {
			return nil, s.failure(method, res.StatusCode, nil)
		}
		s.sessionID = id
	}
	content, _, err := mime.ParseMediaType(res.Header.Get("Content-Type"))
	if err != nil {
		return nil, s.failure(method, res.StatusCode, nil)
	}
	if content == "text/event-stream" {
		body, err = sseResult(body)
		if err != nil {
			return nil, s.failure(method, res.StatusCode, nil)
		}
	} else if content != "application/json" {
		return nil, s.failure(method, res.StatusCode, nil)
	}
	if webvitals.UniqueJSON(body) != nil {
		return nil, s.failure(method, res.StatusCode, nil)
	}
	var result struct {
		JSONRPC string          `json:"jsonrpc"`
		ID      int             `json:"id"`
		Result  json.RawMessage `json:"result"`
		Error   json.RawMessage `json:"error"`
	}
	if json.Unmarshal(body, &result) != nil || result.JSONRPC != "2.0" || result.ID != s.id || len(result.Error) > 0 || len(result.Result) == 0 || bytes.Equal(result.Result, []byte("null")) {
		var failure struct {
			Code *int `json:"code"`
		}
		var code *int
		if json.Unmarshal(result.Error, &failure) == nil && failure.Code != nil && *failure.Code >= -1000000 && *failure.Code <= 1000000 {
			code = failure.Code
		}
		return nil, s.failure(method, res.StatusCode, code)
	}
	return result.Result, nil
}
func sseResult(raw []byte) ([]byte, error) {
	var payload []byte
	events := strings.Split(strings.ReplaceAll(string(raw), "\r\n", "\n"), "\n\n")
	if len(events) > 32 {
		return nil, ErrUnavailable
	}
	for _, event := range events {
		var lines []string
		for _, line := range strings.Split(event, "\n") {
			if strings.HasPrefix(line, "data:") {
				lines = append(lines, strings.TrimPrefix(strings.TrimPrefix(line, "data:"), " "))
			}
		}
		if len(lines) > 0 {
			if payload != nil {
				return nil, ErrUnavailable
			}
			payload = []byte(strings.Join(lines, "\n"))
		}
	}
	if len(payload) == 0 {
		return nil, ErrUnavailable
	}
	return payload, nil
}
func (s *session) initialize(ctx context.Context) error {
	raw, err := s.rpc(ctx, "initialize", map[string]any{"protocolVersion": "2025-03-26", "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "studio-integration-core", "version": "1"}})
	if err != nil {
		return err
	}
	var result struct {
		Protocol string `json:"protocolVersion"`
	}
	if json.Unmarshal(raw, &result) != nil || (result.Protocol != "2025-03-26" && result.Protocol != "2024-11-05" && result.Protocol != "2025-06-18") {
		return s.failure("initialize", 200, nil)
	}
	s.protocol = result.Protocol
	_, err = s.rpc(ctx, "notifications/initialized", map[string]any{})
	return err
}
func safeHTTP(client *http.Client) *http.Client {
	copy := *client
	copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	copy.Jar = nil
	copy.Timeout = 8 * time.Second
	return &copy
}
