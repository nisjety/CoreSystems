package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	stdhttp "net/http"
	"net/url"
	"strings"
	"time"
)

const controlOwnerEffectReservationsPath = "/api/v1/internal/spaces/owner-effect-reservations"

var ErrOwnerEffectReservationDenied = errors.New("Control owner-effect reservation denied")
var ErrOwnerEffectReservationConflict = errors.New("Control owner-effect reservation conflicts with an existing operation")

// ownerEffectReservationCommitment is the content-free, immutable binding
// that Control records. It intentionally excludes the ticket body and the
// signed bearer: Conversation Core retains both at their proper boundaries.
type ownerEffectReservationCommitment struct {
	OperationID      string `json:"operation_id"`
	ActionID         string `json:"action_id"`
	ActionSchemaHash string `json:"action_schema_hash"`
	PayloadDigest    string `json:"payload_digest"`
	IdempotencyKey   string `json:"idempotency_key"`
	DecisionRef      string `json:"decision_ref"`
	GrantRef         string `json:"grant_ref"`
}

func (c ownerEffectReservationCommitment) valid() bool {
	for _, value := range []string{c.OperationID, c.ActionID, c.ActionSchemaHash, c.PayloadDigest, c.IdempotencyKey, c.DecisionRef, c.GrantRef} {
		if value = strings.TrimSpace(value); value == "" || len(value) > 200 {
			return false
		}
	}
	return c.ActionID == "tickets.create" && validRunActionCommitment(c.ActionSchemaHash) && validRunActionCommitment(c.PayloadDigest)
}

type ownerEffectReservationReceipt struct {
	ReservationID string `json:"reservation_id"`
	OperationID   string `json:"operation_id"`
	Status        string `json:"status"`
}

// ControlOwnerEffectReservationCoordinator is the direct, credentialed
// Conversation Core → Control client for owner-effect-reservation-v1. It
// never returns the decision bearer and never stores it; callers may use a
// committed receipt only to enter their own owner-plane transaction.
type ControlOwnerEffectReservationCoordinator struct {
	endpoint     *url.URL
	serviceToken string
	client       *stdhttp.Client
}

func NewControlOwnerEffectReservationCoordinator(baseURL, serviceToken string, allowInsecureLoopback bool) (*ControlOwnerEffectReservationCoordinator, error) {
	endpoint, err := controlServiceEndpoint(baseURL, controlOwnerEffectReservationsPath, allowInsecureLoopback)
	if err != nil {
		return nil, err
	}
	serviceToken = strings.TrimSpace(serviceToken)
	if len(serviceToken) < 32 {
		return nil, fmt.Errorf("Control owner-effect reservation service token is invalid")
	}
	return &ControlOwnerEffectReservationCoordinator{
		endpoint: endpoint, serviceToken: serviceToken,
		client: &stdhttp.Client{Timeout: 5 * time.Second, CheckRedirect: func(_ *stdhttp.Request, _ []*stdhttp.Request) error { return stdhttp.ErrUseLastResponse }},
	}, nil
}

func (c *ControlOwnerEffectReservationCoordinator) Ready() bool {
	return c != nil && c.endpoint != nil && c.client != nil && len(c.serviceToken) >= 32
}

func (c *ControlOwnerEffectReservationCoordinator) Reserve(ctx context.Context, decisionToken string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
	if !c.Ready() || !commitment.valid() || strings.TrimSpace(decisionToken) == "" {
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation coordinator is unavailable")
	}
	return c.request(ctx, stdhttp.MethodPost, "", struct {
		ControlDecisionToken string                           `json:"control_decision_token"`
		Commitment           ownerEffectReservationCommitment `json:"commitment"`
	}{ControlDecisionToken: decisionToken, Commitment: commitment})
}

func (c *ControlOwnerEffectReservationCoordinator) Commit(ctx context.Context, reservationID, decisionToken string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
	if !c.Ready() || strings.TrimSpace(reservationID) == "" || strings.TrimSpace(decisionToken) == "" || !commitment.valid() {
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation coordinator is unavailable")
	}
	return c.request(ctx, stdhttp.MethodPost, "/"+url.PathEscape(strings.TrimSpace(reservationID))+"/commit", struct {
		ControlDecisionToken string                           `json:"control_decision_token"`
		Commitment           ownerEffectReservationCommitment `json:"commitment"`
	}{ControlDecisionToken: decisionToken, Commitment: commitment})
}

func (c *ControlOwnerEffectReservationCoordinator) request(ctx context.Context, method, suffix string, body any) (ownerEffectReservationReceipt, error) {
	payload, err := json.Marshal(body)
	if err != nil {
		return ownerEffectReservationReceipt{}, fmt.Errorf("encode Control owner-effect reservation: %w", err)
	}
	endpoint := *c.endpoint
	endpoint.Path += suffix
	request, err := stdhttp.NewRequestWithContext(ctx, method, endpoint.String(), bytes.NewReader(payload))
	if err != nil {
		return ownerEffectReservationReceipt{}, fmt.Errorf("create Control owner-effect reservation request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", "conversation-core")
	request.Header.Set("X-Service-Token", c.serviceToken)
	response, err := c.client.Do(request)
	if err != nil {
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation request failed: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode == stdhttp.StatusForbidden {
		return ownerEffectReservationReceipt{}, ErrOwnerEffectReservationDenied
	}
	if response.StatusCode == stdhttp.StatusConflict {
		return ownerEffectReservationReceipt{}, ErrOwnerEffectReservationConflict
	}
	if response.StatusCode != stdhttp.StatusOK {
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation is unavailable")
	}
	var envelope struct {
		Data ownerEffectReservationReceipt `json:"data"`
	}
	decoder := json.NewDecoder(io.LimitReader(response.Body, 4*1024))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&envelope); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation response is invalid")
	}
	if strings.TrimSpace(envelope.Data.ReservationID) == "" || strings.TrimSpace(envelope.Data.OperationID) == "" {
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation response is incomplete")
	}
	switch envelope.Data.Status {
	case "reserved", "committed", "cancelled":
		return envelope.Data, nil
	default:
		return ownerEffectReservationReceipt{}, fmt.Errorf("Control owner-effect reservation response has invalid status")
	}
}

func controlServiceEndpoint(baseURL, path string, allowInsecureLoopback bool) (*url.URL, error) {
	parsed, err := url.Parse(strings.TrimSpace(baseURL))
	if err != nil || parsed.Scheme == "" || parsed.Host == "" || (parsed.Scheme != "http" && parsed.Scheme != "https") ||
		parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return nil, fmt.Errorf("Control service URL is invalid")
	}
	if parsed.Scheme == "http" && (!allowInsecureLoopback || !isIPLoopback(parsed.Hostname())) {
		return nil, fmt.Errorf("Control service must use HTTPS; plaintext is permitted only for an explicitly enabled IP-loopback development endpoint")
	}
	parsed.Path = strings.TrimRight(parsed.Path, "/") + path
	return parsed, nil
}
