package cron

import (
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// contractVectorText is the canonical text shared with mp-contracts
// scheduled_contract.rs. Keep the two in step; the digest pin is over these bytes.
const contractVectorText = `{"expires_at":"2026-11-23T10:00:00Z","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}`

// contractGoVectorDigest is pinned to the sha256 of contractVectorText.
const contractGoVectorDigest = "sha256:3d4828c85b976a25447098498e5077de75aa531a0e64aebe2d68bfc8f7953d55"

func TestParseScheduleContractAcceptsTheVector(t *testing.T) {
	contract, err := ParseScheduleContract(contractVectorText)
	if err != nil {
		t.Fatalf("vector refused: %v", err)
	}
	if len(contract.Tools) != 1 || contract.MaxRounds != 6 {
		t.Fatalf("unexpected contract: %+v", contract)
	}
}

func TestParseScheduleContractRefusesMalformedContracts(t *testing.T) {
	cases := map[string]string{
		"unknown key":         strings.Replace(contractVectorText, `"v":1`, `"v":1,"extra":true`, 1),
		"trailing data":       contractVectorText + ` {}`,
		"version 2":           strings.Replace(contractVectorText, `"v":1`, `"v":2`, 1),
		"no tools":            strings.Replace(contractVectorText, `"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}]`, `"tools":[]`, 1),
		"class above ceiling": strings.Replace(contractVectorText, `"class":"read"`, `"class":"external_write"`, 1),
		"non-delegable tool":  strings.Replace(contractVectorText, `"tool":"web_search"`, `"tool":"mcp__github__search"`, 1),
		"offset expiry":       strings.Replace(contractVectorText, `"expires_at":"2026-11-23T10:00:00Z"`, `"expires_at":"2026-11-23T10:00:00+00:00"`, 1),
		"rounds over max":     strings.Replace(contractVectorText, `"max_rounds":6`, `"max_rounds":9`, 1),
		"tokens over max":     strings.Replace(contractVectorText, `"max_tokens_per_round":2048`, `"max_tokens_per_round":4097`, 1),
		"not json":            `{"v":1`,
	}
	for name, text := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := ParseScheduleContract(text); err == nil {
				t.Fatalf("%s was accepted", name)
			}
		})
	}
}

func TestContractDigestIsPinnedToTheExactText(t *testing.T) {
	if got := ContractDigest(contractVectorText); got != contractGoVectorDigest {
		t.Fatalf("digest drifted from the cross-plane vector: %s", got)
	}
	reordered := strings.Replace(contractVectorText, `"max_rounds":6,"max_tokens_per_round":2048`, `"max_tokens_per_round":2048,"max_rounds":6`, 1)
	if ContractDigest(reordered) == contractGoVectorDigest {
		t.Fatal("the digest must cover the exact stored bytes")
	}
}

func TestCheckContractWindowRefusesExpiredAndOverlong(t *testing.T) {
	contract, err := ParseScheduleContract(contractVectorText)
	if err != nil {
		t.Fatal(err)
	}
	expired := time.Date(2026, time.November, 24, 0, 0, 0, 0, time.UTC)
	if err := CheckContractWindow(contract, expired); err == nil {
		t.Fatal("expired contract was accepted")
	}
	start := time.Date(2026, time.October, 24, 0, 0, 0, 0, time.UTC)
	contract.ExpiresAt = start.AddDate(0, 0, ContractMaxDays+1).Format(contractTimeLayout)
	if err := CheckContractWindow(contract, start); err == nil {
		t.Fatal("contract beyond the maximum window was accepted")
	}
	contract.ExpiresAt = start.AddDate(0, 0, ContractDefaultDays).Format(contractTimeLayout)
	if err := CheckContractWindow(contract, start); err != nil {
		t.Fatalf("a 30 day contract was refused: %v", err)
	}
}

func TestVerifyContractAgainstRegistry(t *testing.T) {
	contract, err := ParseScheduleContract(contractVectorText)
	if err != nil {
		t.Fatal(err)
	}
	read := DeclaredTool{ToolName: "web_search", CapabilityID: "cap.tool.web_search",
		SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone}
	if err := VerifyContractAgainstRegistry(contract, []DeclaredTool{read}); err != nil {
		t.Fatalf("a read tool with no floor was refused: %v", err)
	}
	cases := map[string][]DeclaredTool{
		"not declared": nil,
		"write class": {{ToolName: "web_search", CapabilityID: "cap.tool.web_search",
			SideEffectClass: models.SideEffectExternalWrite, ApprovalFloor: models.ApprovalFloorNone}},
		"floor above none": {{ToolName: "web_search", CapabilityID: "cap.tool.web_search",
			SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk}},
		"capability mismatch": {{ToolName: "web_search", CapabilityID: "cap.other",
			SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone}},
		"stricter row hidden by a permissive one": {read, {ToolName: "web_search", CapabilityID: "cap.tool.web_search",
			SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk}},
		"refined row": {{ToolName: "web_search", CapabilityID: "cap.tool.web_search",
			SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, CallRefinement: models.CallRefinementProviderOperation}},
	}
	for name, declared := range cases {
		t.Run(name, func(t *testing.T) {
			if err := VerifyContractAgainstRegistry(contract, declared); err == nil {
				t.Fatalf("%s was accepted", name)
			}
		})
	}
}

func TestDelegableAdmitsReadOnlyAndNonDelegableNamesAreRefused(t *testing.T) {
	if !DelegableClass("read") {
		t.Fatal("read must be delegable")
	}
	for _, class := range []string{"sandbox_compute", "context_write", "external_write", "unclassified", ""} {
		if DelegableClass(class) {
			t.Fatalf("%q must not be delegable", class)
		}
	}
	for _, name := range []string{"browser.open", "browser_agent", "process_start", "mcp__x__y", "shell", "schedule.create"} {
		if !IsNonDelegableTool(name) {
			t.Fatalf("%s must be non-delegable", name)
		}
	}
	if IsNonDelegableTool("web_search") {
		t.Fatal("web_search must stay delegable by name")
	}
}
