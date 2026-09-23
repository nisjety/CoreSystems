// Package agentmemory implements a Redis Agent Memory (agent-memory-server)
// backend for letta-bridge's long-term memory tier. It satisfies the
// server.Store interface, upgrading the in-memory substring stub to managed
// vector (semantic) recall. The server generates embeddings, so this client
// only ships text plus scoping metadata.
//
// API: https://redis.github.io/agent-memory-server/ (REST, Bearer auth).
package agentmemory

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

	"github.com/triodelab/model-plane/services/letta-bridge/internal/memstore"
)

const (
	defaultTimeout = 5 * time.Second
	memoryType     = "semantic"
)

// Config configures the client. BaseURL is required; APIKey is sent as a
// Bearer token when set (omit it only when the server runs with
// DISABLE_AUTH=true). New returns ok=false when BaseURL is empty.
type Config struct {
	BaseURL string
	APIKey  string
	Timeout time.Duration
}

// Client talks to the Agent Memory Server long-term memory REST API.
type Client struct {
	http    *http.Client
	baseURL string
	apiKey  string
}

// New builds a Client. ok is false when required config is missing.
func New(cfg Config) (client *Client, ok bool) {
	if cfg.BaseURL == "" {
		return nil, false
	}
	timeout := cfg.Timeout
	if timeout <= 0 {
		timeout = defaultTimeout
	}
	return &Client{
		http:    &http.Client{Timeout: timeout},
		baseURL: strings.TrimRight(cfg.BaseURL, "/"),
		apiKey:  cfg.APIKey,
	}, true
}

type memoryRecord struct {
	ID         string   `json:"id"`
	Text       string   `json:"text"`
	Namespace  string   `json:"namespace,omitempty"`
	SessionID  string   `json:"session_id,omitempty"`
	Topics     []string `json:"topics,omitempty"`
	MemoryType string   `json:"memory_type,omitempty"`
	UserID     string   `json:"user_id,omitempty"`
	// Session Core already extracted and authorized this exact fact. Letting
	// the vector backend extract it again creates untracked IDs that survive
	// editing/deleting the original and can change its scope or meaning.
	DiscreteMemoryExtracted string `json:"discrete_memory_extracted"`
}

type createRequest struct {
	Memories []memoryRecord `json:"memories"`
}

type eqFilter struct {
	Eq string `json:"eq"`
}

type anyFilter struct {
	Any []string `json:"any"`
}

type gteFilter struct {
	Gte int64 `json:"gte"`
}

type searchRequest struct {
	Text       string     `json:"text"`
	SearchMode string     `json:"search_mode,omitempty"`
	Limit      int        `json:"limit,omitempty"`
	Namespace  *eqFilter  `json:"namespace,omitempty"`
	SessionID  *eqFilter  `json:"session_id,omitempty"`
	Topics     *anyFilter `json:"topics,omitempty"`
	MemoryType *eqFilter  `json:"memory_type,omitempty"`
	CreatedAt  *gteFilter `json:"created_at,omitempty"`
	UserID     *eqFilter  `json:"user_id,omitempty"`
}

type searchResult struct {
	ID        string   `json:"id"`
	Text      string   `json:"text"`
	SessionID string   `json:"session_id"`
	Topics    []string `json:"topics"`
	Dist      float64  `json:"dist"`
	UpdatedAt string   `json:"updated_at"`
	CreatedAt string   `json:"created_at"`
}

type searchResponse struct {
	Memories []searchResult `json:"memories"`
	Total    int            `json:"total"`
}

// Put stores a long-term memory. orgID maps to the namespace so memories never
// cross tenants; threadID maps to session_id and topic to a memory topic.
// userID, when non-empty, tags the record so a later List/Delete scoped to
// that user can find it -- see List and Delete below.
func (c *Client) Put(ctx context.Context, orgID, threadID, topic, memoryID, userID, content string) (*memstore.Record, error) {
	var topics []string
	if topic != "" {
		topics = []string{topic}
	}
	body := createRequest{Memories: []memoryRecord{{
		ID:                      memoryID,
		Text:                    content,
		Namespace:               orgID,
		SessionID:               threadID,
		Topics:                  topics,
		MemoryType:              memoryType,
		UserID:                  userID,
		DiscreteMemoryExtracted: "t",
	}}}
	if _, err := c.post(ctx, "/v1/long-term-memory/", body); err != nil {
		return nil, err
	}
	return &memstore.Record{
		OrgID:     orgID,
		ThreadID:  threadID,
		Topic:     topic,
		MemoryID:  memoryID,
		Content:   content,
		UpdatedAt: time.Now().UTC(),
	}, nil
}

// Search runs a semantic search over long-term memory, scoped to the org
// (namespace) and optionally the thread and topics.
func (c *Client) Search(ctx context.Context, orgID, threadID, userID, query string, topicFilter []string, updatedAfter time.Time, topK int32) ([]memstore.Hit, error) {
	body := searchRequest{
		Text:       query,
		Namespace:  &eqFilter{Eq: orgID},
		MemoryType: &eqFilter{Eq: memoryType},
	}
	if topK > 0 {
		body.Limit = int(topK)
	}
	if threadID != "" {
		body.SessionID = &eqFilter{Eq: threadID}
	}
	if userID != "" {
		// NARROWER than the rule pgstore and session-core apply.
		//
		// The correct rule is "memories owned by this user OR owned by nobody"
		// (org/workspace/policy scope). The upstream filter is equality-only, and
		// the search response carries no `user_id`, so neither the request nor a
		// client-side pass can express the OR.
		//
		// So this narrows: only the user's own memories come back, and org-level
		// ones are lost from the semantic tier. That is the correct direction to
		// fail for a scoping filter -- under-recall is a quality regression,
		// over-recall would be another user's private memory in someone's chat --
		// but it is a real gap, and it is a PROMOTION BLOCKER for this backend
		// (plan item 2.3): it cannot replace pgstore until the OR is
		// expressible, either by an upstream filter that supports it or by the
		// response carrying `user_id` so this client can filter.
		body.UserID = &eqFilter{Eq: userID}
	}
	if len(topicFilter) > 0 {
		body.Topics = &anyFilter{Any: topicFilter}
	}
	if !updatedAfter.IsZero() {
		body.CreatedAt = &gteFilter{Gte: updatedAfter.Unix()}
	}

	raw, err := c.post(ctx, "/v1/long-term-memory/search", body)
	if err != nil {
		return nil, err
	}
	var resp searchResponse
	if err := json.Unmarshal(raw, &resp); err != nil {
		return nil, fmt.Errorf("agentmemory decode search response: %w", err)
	}

	hits := make([]memstore.Hit, 0, len(resp.Memories))
	for _, m := range resp.Memories {
		topicOut := ""
		if len(m.Topics) > 0 {
			topicOut = m.Topics[0]
		}
		hits = append(hits, memstore.Hit{
			MemoryID:  m.ID,
			ThreadID:  m.SessionID,
			Topic:     topicOut,
			Score:     scoreFromDist(m.Dist),
			Content:   m.Text,
			UpdatedAt: parseTime(m.UpdatedAt, m.CreatedAt),
		})
	}
	return hits, nil
}

// List enumerates long-term memories owned by a user, across every session,
// scoped to the org (namespace). Unlike Search this is never session-scoped --
// it backs a "what do you remember about me" surface, so a session/thread
// filter would defeat the point.
//
// It reuses the search endpoint (there is no dedicated list-all endpoint) with
// an empty query and search_mode "keyword", which skips the embedding call a
// semantic-mode search would otherwise make for a query with no text --
// exactly the round trip today's fix to LETTA_TIMEOUT_MS budgeted for, so
// enumerating a user's memories should not gamble with that budget for no
// benefit: there is no relevance to rank against an empty query.
func (c *Client) List(ctx context.Context, orgID, userID string, topK int32) ([]memstore.Hit, error) {
	if userID == "" {
		return nil, fmt.Errorf("agentmemory list: userID is required")
	}
	body := searchRequest{
		SearchMode: "keyword",
		Namespace:  &eqFilter{Eq: orgID},
		MemoryType: &eqFilter{Eq: memoryType},
		UserID:     &eqFilter{Eq: userID},
	}
	if topK > 0 {
		body.Limit = int(topK)
	}

	raw, err := c.post(ctx, "/v1/long-term-memory/search", body)
	if err != nil {
		return nil, err
	}
	var resp searchResponse
	if err := json.Unmarshal(raw, &resp); err != nil {
		return nil, fmt.Errorf("agentmemory decode list response: %w", err)
	}

	hits := make([]memstore.Hit, 0, len(resp.Memories))
	for _, m := range resp.Memories {
		topicOut := ""
		if len(m.Topics) > 0 {
			topicOut = m.Topics[0]
		}
		hits = append(hits, memstore.Hit{
			MemoryID:  m.ID,
			ThreadID:  m.SessionID,
			Topic:     topicOut,
			Score:     1,
			Content:   m.Text,
			UpdatedAt: parseTime(m.UpdatedAt, m.CreatedAt),
		})
	}
	return hits, nil
}

// Delete removes a single long-term memory by id.
//
// orgID and userID are accepted for interface parity and observability, but
// the agent-memory-server delete endpoint takes only an id (see
// redis.github.io/agent-memory-server) -- it has no namespace/user filter to
// enforce here. This is safe in practice because the only caller,
// session-core's MemoryGrpc, always verifies ownership of memoryID against
// its own `agent_memory` table (scoped to org_id + owner) before ever
// reaching this call, and today's ids are correlated 1:1 across both stores
// (see memory.proto's IndexMemoryRequest.memory_id) -- so a caller can only
// ever cause this delete to run for an id they already own locally.
func (c *Client) Delete(ctx context.Context, orgID, userID, memoryID string) (bool, error) {
	_ = orgID
	_ = userID
	if memoryID == "" {
		return false, fmt.Errorf("agentmemory delete: memoryID is required")
	}
	values := url.Values{}
	values.Add("memory_ids", memoryID)
	if _, err := c.delete(ctx, "/v1/long-term-memory?"+values.Encode()); err != nil {
		return false, err
	}
	return true, nil
}

// scoreFromDist maps a vector distance (0 = identical) to a [0,1] similarity
// score so it lines up with the in-memory store's scoring convention.
func scoreFromDist(dist float64) float32 {
	score := 1 - dist
	if score < 0 {
		score = 0
	}
	if score > 1 {
		score = 1
	}
	return float32(score)
}

func parseTime(primary, fallback string) time.Time {
	for _, s := range []string{primary, fallback} {
		if s == "" {
			continue
		}
		if t, err := time.Parse(time.RFC3339, s); err == nil {
			return t.UTC()
		}
	}
	return time.Now().UTC()
}

func (c *Client) post(ctx context.Context, path string, body any) ([]byte, error) {
	buf, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("agentmemory marshal: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+path, bytes.NewReader(buf))
	if err != nil {
		return nil, fmt.Errorf("agentmemory new request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	if c.apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+c.apiKey)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("agentmemory POST %s: %w", path, err)
	}
	defer func() { _ = resp.Body.Close() }()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("agentmemory read body: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("agentmemory POST %s: status %d: %s", path, resp.StatusCode, strings.TrimSpace(string(data)))
	}
	return data, nil
}

func (c *Client) delete(ctx context.Context, path string) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, c.baseURL+path, nil)
	if err != nil {
		return nil, fmt.Errorf("agentmemory new request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	if c.apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+c.apiKey)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("agentmemory DELETE %s: %w", path, err)
	}
	defer func() { _ = resp.Body.Close() }()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("agentmemory read body: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("agentmemory DELETE %s: status %d: %s", path, resp.StatusCode, strings.TrimSpace(string(data)))
	}
	return data, nil
}
