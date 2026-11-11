package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// zdrSpendEventType is the event_type of a ZDR spend row: the one content-free
// kind of envelope a zero-data-retention turn may put on the usage ledger
// (DEF-Z-06b, FU-30). It is the wire value of model-gateway's
// mp_contracts::zdr_spend::EVENT_TYPE.
//
// A ZDR turn's spend used to arrive as an ordinary usage envelope labelled
// zdr=false so that it passed this service's refusal of ZDR usage. It now keeps
// zdr=true and names its kind, and this service, which owns the ledger, admits
// exactly that kind with exactly the allowlisted fields (ADR-0005: the owner
// enforces). Every other zdr=true usage envelope is still refused.
const zdrSpendEventType = "ZDR_SPEND"

// zdrSpendCurrency is the only currency a spend row states its amount in.
const zdrSpendCurrency = "USD"

// zdrSpendPayloadFields is every payload key a ZDR spend row may carry. It
// mirrors mp_contracts::zdr_spend::PAYLOAD_FIELDS. A payload with any other key
// is refused whole; nothing is added to this list without an owner decision.
var zdrSpendPayloadFields = map[string]struct{}{
	"request_id":                  {},
	"org_id":                      {},
	"user_id":                     {},
	"model":                       {},
	"provider_used":               {},
	"input_tokens":                {},
	"output_tokens":               {},
	"cache_read_input_tokens":     {},
	"cache_creation_input_tokens": {},
	"cost_usd":                    {},
	"currency":                    {},
	"cost_basis":                  {},
	"retention_class":             {},
}

// zdrSpendRequiredFields are the payload keys a ZDR spend row must carry. The
// amount, the cache counts and the cost basis are optional: an answer whose
// model has no known price states no amount.
var zdrSpendRequiredFields = []string{
	"request_id", "org_id", "user_id", "model", "provider_used",
	"input_tokens", "output_tokens", "currency", "retention_class",
}

// zdrSpendPayload is the typed view of a ZDR spend row's payload. It holds the
// allowlisted fields and nothing else, so a decode into it cannot carry content.
type zdrSpendPayload struct {
	RequestID                string   `json:"request_id"`
	OrgID                    string   `json:"org_id"`
	UserID                   string   `json:"user_id"`
	Model                    string   `json:"model"`
	ProviderUsed             string   `json:"provider_used"`
	InputTokens              int64    `json:"input_tokens"`
	OutputTokens             int64    `json:"output_tokens"`
	CacheReadInputTokens     int64    `json:"cache_read_input_tokens"`
	CacheCreationInputTokens int64    `json:"cache_creation_input_tokens"`
	CostUSD                  *float64 `json:"cost_usd"`
	Currency                 string   `json:"currency"`
	CostBasis                string   `json:"cost_basis"`
	RetentionClass           string   `json:"retention_class"`
}

// decodeZDRSpendPayload admits a ZDR spend row's payload or refuses it whole.
// It refuses a key outside zdrSpendPayloadFields (matched exactly, since Go's
// own field matching is case-insensitive), a missing required key, a value of
// another type, a currency other than USD, and a retention class other than
// zdr or zdr_waived. The result is the usage payload the ledger entry is built
// from; it carries no run id and no cache-miss reason, which a spend row cannot
// state.
func decodeZDRSpendPayload(raw json.RawMessage) (usagePayload, error) {
	var keys map[string]json.RawMessage
	if err := json.Unmarshal(raw, &keys); err != nil || keys == nil {
		return usagePayload{}, fmt.Errorf("zdr spend payload must be an object")
	}
	for key := range keys {
		if _, ok := zdrSpendPayloadFields[key]; !ok {
			return usagePayload{}, fmt.Errorf("zdr spend payload carries a field outside the allowlist")
		}
	}
	for _, key := range zdrSpendRequiredFields {
		if _, ok := keys[key]; !ok {
			return usagePayload{}, fmt.Errorf("zdr spend payload is missing %s", key)
		}
	}
	var p zdrSpendPayload
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&p); err != nil {
		return usagePayload{}, fmt.Errorf("decode zdr spend payload: %w", err)
	}
	for _, field := range []struct{ name, value string }{
		{"request_id", p.RequestID}, {"org_id", p.OrgID}, {"user_id", p.UserID}, {"model", p.Model},
	} {
		if field.value == "" {
			return usagePayload{}, fmt.Errorf("zdr spend payload is missing %s", field.name)
		}
	}
	for _, value := range []string{p.RequestID, p.OrgID, p.UserID, p.Model, p.ProviderUsed} {
		if !utf8.ValidString(value) || strings.IndexFunc(value, unicode.IsControl) >= 0 {
			return usagePayload{}, fmt.Errorf("zdr spend payload carries a non-canonical id")
		}
	}
	if p.Currency != zdrSpendCurrency {
		return usagePayload{}, fmt.Errorf("zdr spend currency must be %s", zdrSpendCurrency)
	}
	if p.RetentionClass != "zdr" && p.RetentionClass != "zdr_waived" {
		return usagePayload{}, fmt.Errorf("zdr spend retention_class must be zdr or zdr_waived")
	}
	if basis, ok := ledger.NormalizeCostBasis(p.CostBasis); !ok || basis != p.CostBasis {
		return usagePayload{}, fmt.Errorf("zdr spend cost_basis is not a known basis")
	}
	out := usagePayload{
		RequestID:                p.RequestID,
		OrgID:                    p.OrgID,
		UserID:                   p.UserID,
		Model:                    p.Model,
		ProviderUsed:             p.ProviderUsed,
		InputTokens:              p.InputTokens,
		OutputTokens:             p.OutputTokens,
		CacheReadInputTokens:     p.CacheReadInputTokens,
		CacheCreationInputTokens: p.CacheCreationInputTokens,
		CostBasis:                p.CostBasis,
	}
	if p.CostUSD != nil {
		out.CostUSD = *p.CostUSD
	}
	return out, nil
}
