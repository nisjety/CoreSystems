package taskexec

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// contractTemplate returns a canonical template carrying an approval_contract
// that expires at the given instant, as the create path stores it.
func contractTemplate(t *testing.T, now, expires time.Time) []byte {
	t.Helper()
	text, err := json.Marshal(cron.ScheduleContract{
		V: 1,
		Tools: []cron.ContractTool{{
			Tool: "web_search", Capability: "web_search", Class: "read",
			ApprovedAt: now.UTC().Format("2006-01-02T15:04:05Z"),
		}},
		ExpiresAt:         expires.UTC().Format("2006-01-02T15:04:05Z"),
		MaxRounds:         6,
		MaxTokensPerRound: 2048,
	})
	if err != nil {
		t.Fatalf("marshal contract: %v", err)
	}
	template, err := json.Marshal(map[string]string{
		"goal":              "summarise the day",
		"approval_contract": string(text),
	})
	if err != nil {
		t.Fatalf("marshal template: %v", err)
	}
	return template
}

func TestRefuseExpiredContractAllowsLiveAndContractFreeTemplates(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	if err := refuseExpiredContract([]byte(`{"goal":"no contract"}`), now); err != nil {
		t.Fatalf("contract-free template refused: %v", err)
	}
	if err := refuseExpiredContract(contractTemplate(t, now, now.Add(time.Hour)), now); err != nil {
		t.Fatalf("live contract refused: %v", err)
	}
}

func TestRefuseExpiredContractStopsFireAfterWindow(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	err := refuseExpiredContract(contractTemplate(t, now, now.Add(-time.Minute)), now)
	if err == nil || !strings.Contains(err.Error(), "contract_expired") {
		t.Fatalf("expired contract must refuse the fire with contract_expired, got %v", err)
	}
}

func TestRefuseExpiredContractRefusesMalformedContract(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	err := refuseExpiredContract([]byte(`{"approval_contract":"{\"v\":1,\"surprise\":true}"}`), now)
	if err == nil || !strings.Contains(err.Error(), "contract_invalid") {
		t.Fatalf("malformed contract must fail closed, got %v", err)
	}
}
