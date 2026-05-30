// Package activities implements Temporal activities that bridge workflows
// to Quarry Runtime (page execution) and Quarry Control (checkpoints/events).
//
// All activities translate transport and HTTP errors into classified
// [errs.Error] values and return [temporal.ApplicationError] results so that
// workflow-side retry policies honor Quarry's non-retryable categories.
package activities

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// Config holds endpoint URLs and bearer tokens for the upstream services.
type Config struct {
	RuntimeBaseURL   string
	ControlBaseURL   string
	RuntimeAuthToken string
	ControlAuthToken string
	HTTPTimeout      time.Duration
}

// Activities wires HTTP calls into Temporal activity methods.
type Activities struct {
	cfg  Config
	http *http.Client
}

// New returns an Activities instance with a configured HTTP client.
func New(cfg Config) *Activities {
	timeout := cfg.HTTPTimeout
	if timeout <= 0 {
		timeout = 2 * time.Minute
	}
	return &Activities{
		cfg:  cfg,
		http: &http.Client{Timeout: timeout},
	}
}

// RunPageInput is the input for a single page execution.
type RunPageInput struct {
	RunID string `json:"run_id"`
	URL   string `json:"url"`
}

// RunPageResult is returned by the runtime after a successful page execution.
type RunPageResult struct {
	RunID       string   `json:"run_id"`
	Status      uint16   `json:"status"`
	Fingerprint string   `json:"fingerprint"`
	Links       []string `json:"links"`
}

// RunPage executes a single page via Quarry Runtime.
func (a *Activities) RunPage(ctx context.Context, in RunPageInput) (RunPageResult, error) {
	const op = "activities.RunPage"
	body, err := json.Marshal(map[string]string{"url": in.URL})
	if err != nil {
		return RunPageResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := strings.TrimRight(a.cfg.RuntimeBaseURL, "/") + "/v1/internal/run_page"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return RunPageResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", idempotencyKey(in.RunID, in.URL))
	setAuth(req, a.cfg.RuntimeAuthToken)

	resp, err := a.http.Do(req)
	if err != nil {
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return RunPageResult{}, errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return RunPageResult{}, errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}

	var out RunPageResult
	if err := json.Unmarshal(respBody, &out); err != nil {
		return RunPageResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("decode response: %w", err)).Temporal()
	}
	out.RunID = in.RunID
	return out, nil
}

// CheckpointInput is the input for a run checkpoint.
type CheckpointInput struct {
	RunID    string `json:"run_id"`
	Visited  uint32 `json:"visited"`
	Frontier uint32 `json:"frontier"`
}

// Checkpoint persists a run progress checkpoint via Quarry Control.
func (a *Activities) Checkpoint(ctx context.Context, in CheckpointInput) error {
	const op = "activities.Checkpoint"
	body, err := json.Marshal(in)
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := fmt.Sprintf("%s/v1/runs/%s/checkpoints", strings.TrimRight(a.cfg.ControlBaseURL, "/"), in.RunID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", idempotencyKey(in.RunID, strconv.FormatUint(uint64(in.Visited), 10), strconv.FormatUint(uint64(in.Frontier), 10)))
	setAuth(req, a.cfg.ControlAuthToken)

	resp, err := a.http.Do(req)
	if err != nil {
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(resp.Body)
		return errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	return nil
}

// EmitEvent publishes a Quarry event via Quarry Control using the event's own
// IdempotencyKey as the HTTP Idempotency-Key header.
func (a *Activities) EmitEvent(ctx context.Context, runID string, event quarrycontracts.Event) error {
	const op = "activities.EmitEvent"
	body, err := json.Marshal(event)
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := fmt.Sprintf("%s/v1/runs/%s/events", strings.TrimRight(a.cfg.ControlBaseURL, "/"), runID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	if event.IdempotencyKey != "" {
		req.Header.Set("Idempotency-Key", event.IdempotencyKey)
	}
	setAuth(req, a.cfg.ControlAuthToken)

	resp, err := a.http.Do(req)
	if err != nil {
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(resp.Body)
		return errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	return nil
}

// setAuth attaches a bearer token if non-empty.
func setAuth(req *http.Request, token string) {
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
}

// idempotencyKey returns a deterministic SHA-256 hex digest of the joined parts.
func idempotencyKey(parts ...string) string {
	sum := sha256.Sum256([]byte(strings.Join(parts, ":")))
	return hex.EncodeToString(sum[:])
}
