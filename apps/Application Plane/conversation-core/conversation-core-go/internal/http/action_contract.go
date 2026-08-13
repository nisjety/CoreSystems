package http

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"

	"github.com/gin-gonic/gin"
)

// OwnerActionContract is an owner-published execution contract. It is not a
// browser capability declaration: the owner is still responsible for exact
// authorization immediately before the effect. The BFF may only surface an
// entry after intersecting this contract with the caller's current authority.
type OwnerActionContract struct {
	ActionID                string          `json:"action_id"`
	ContractVersion         string          `json:"contract_version"`
	OwnerPlane              string          `json:"owner_plane"`
	EligibleActorTypes      []string        `json:"eligible_actor_types"`
	RequiredServiceIdentity string          `json:"required_service_identity"`
	RequiredDelegation      string          `json:"required_delegation"`
	Risk                    string          `json:"risk"`
	RequiresApproval        bool            `json:"requires_approval"`
	Reversible              bool            `json:"reversible"`
	Idempotency             string          `json:"idempotency"`
	ReceiptContract         string          `json:"receipt_contract"`
	HTTPMethod              string          `json:"http_method"`
	Path                    string          `json:"path"`
	InputSchema             json.RawMessage `json:"input_schema"`
	SchemaSHA256            string          `json:"schema_sha256"`
}

type ownerActionContractCatalog struct {
	CatalogVersion string                `json:"catalog_version"`
	Actions        []OwnerActionContract `json:"actions"`
}

var ticketCreateInputSchema = json.RawMessage(`{
  "type":"object",
  "required":["conversation_id","idempotency_key"],
  "properties":{
    "conversation_id":{"type":"string","minLength":1},
    "idempotency_key":{"type":"string","minLength":1,"maxLength":200},
    "work_type":{"enum":["customer_case","internal_work","incident"]},
    "priority":{"enum":["low","normal","high","urgent"]},
    "severity":{"enum":["low","medium","high","critical"]},
    "category":{"type":"string","maxLength":80},
    "intent":{"type":"string","maxLength":120}
  }
}`)

func ticketCreateActionContract() OwnerActionContract {
	schema := append(json.RawMessage(nil), ticketCreateInputSchema...)
	digest := canonicalSchemaSHA256(schema)
	return OwnerActionContract{
		ActionID:                "tickets.create",
		ContractVersion:         "tickets.create/v1",
		OwnerPlane:              "application",
		EligibleActorTypes:      []string{"human"},
		RequiredServiceIdentity: "verevon-gateway",
		RequiredDelegation:      "verified_user_org_role",
		Risk:                    "medium",
		RequiresApproval:        false,
		Reversible:              true,
		Idempotency:             "caller_supplied",
		ReceiptContract:         "durable_owner_receipt",
		HTTPMethod:              http.MethodPost,
		Path:                    "/api/v1/tickets",
		InputSchema:             schema,
		SchemaSHA256:            digest,
	}
}

// canonicalSchemaSHA256 hashes JSON after parsing and re-encoding it. This
// makes the cross-plane contract identity insensitive to source whitespace or
// key ordering, while any semantic schema change yields a different digest.
func canonicalSchemaSHA256(schema json.RawMessage) string {
	var value any
	if err := json.Unmarshal(schema, &value); err != nil {
		return ""
	}
	canonical, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	digest := sha256.Sum256(canonical)
	return "sha256:" + hex.EncodeToString(digest[:])
}

// ListActionContracts returns only contracts owned by Conversation Core. This
// route is intentionally service-to-service: it cannot be used by a browser
// or a model runtime as an authority grant. The verified gateway later derives
// the caller-specific view and owner planes continue to enforce every effect.
func (h *Handler) ListActionContracts(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"data": ownerActionContractCatalog{
		CatalogVersion: "conversation-core/v1",
		Actions:        []OwnerActionContract{ticketCreateActionContract()},
	}})
}
