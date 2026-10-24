package api

import (
	"context"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

const contractTemplateText = `{"expires_at":"2026-11-23T10:00:00Z","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}`

func contractTemplate(text string) map[string]any {
	return map[string]any{"goal": "summarise the week", "approval_contract": text}
}

func TestCreateIntentForTemplateWithoutContractIsContractFree(t *testing.T) {
	h := NewCronHandler(nil)
	got, err := h.createIntentFor(context.Background(), "org-1", map[string]any{"goal": "x"}, cron.CreateIntent{ScheduleID: "s-1"})
	if err != nil {
		t.Fatal(err)
	}
	if got.ContractDigest != "" || got.ContractExpiresAt != "" {
		t.Fatalf("a contract-free template acquired contract fields: %+v", got)
	}
}

func TestCreateIntentForRefusesContractWhileDisabled(t *testing.T) {
	h := NewCronHandler(nil)
	if _, err := h.createIntentFor(context.Background(), "org-1", contractTemplate(contractTemplateText), cron.CreateIntent{}); err == nil {
		t.Fatal("a contract was accepted with SCHEDULE_CONTRACT_ENABLED off")
	}
}

func TestCreateIntentForRefusesNonTextContract(t *testing.T) {
	h := NewCronHandler(nil).WithScheduleContract(true)
	template := map[string]any{"goal": "x", "approval_contract": map[string]any{"v": 1}}
	if _, err := h.createIntentFor(context.Background(), "org-1", template, cron.CreateIntent{}); err == nil {
		t.Fatal("a nested-object contract was accepted; the contract must be JSON text")
	}
}

func TestCreateIntentForRefusesMalformedContractBeforeRegistry(t *testing.T) {
	h := NewCronHandler(nil).WithScheduleContract(true)
	text := strings.Replace(contractTemplateText, `"max_rounds":6`, `"max_rounds":99`, 1)
	_, err := h.createIntentFor(context.Background(), "org-1", contractTemplate(text), cron.CreateIntent{})
	if err == nil || !strings.Contains(err.Error(), "max_rounds") {
		t.Fatalf("an out-of-range contract was not refused on its shape: %v", err)
	}
}
