package mpv1

// Wire parity for RunAgentResponse's typed failure (fields 6, 7 and 8).
//
// execution-core (Rust) writes them and model-gateway (Rust) reads them, but
// the Go services decode the same message, and a field renumbered or retyped on
// one side would not fail anything: the reader would see an unset field. The
// same bytes are pinned in
// rust/crates/mp-contracts/tests/proto_wire_parity_execution.rs.
//
// retry_after_ms is optional: a wait the provider did not give is absent on the
// wire, and a wait of zero is present and zero.

import (
	"encoding/hex"
	"testing"

	"google.golang.org/protobuf/proto"
)

const (
	usageLimitWireHex = "0a066661696c65641215546869732072756e207761732073746f707065642e18023218737562736372697074696f6e5f75736167655f6c696d697438a0d9f5184214323032362d30392d32365430313a32313a30305a"
	zeroWaitWireHex   = "0a066661696c65641215546869732072756e207761732073746f707065642e1802320c726174655f6c696d697465643800"
	noTimingWireHex   = "0a066661696c65641215546869732072756e207761732073746f707065642e1802321c737562736372697074696f6e5f7265617574685f7265717569726564"
)

func decodeRunAgentResponse(t *testing.T, wireHex string) *RunAgentResponse {
	t.Helper()
	raw, err := hex.DecodeString(wireHex)
	if err != nil {
		t.Fatalf("bad golden hex: %v", err)
	}
	var decoded RunAgentResponse
	if err := proto.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	return &decoded
}

func TestRunAgentResponseTypedFailureDecodesFromGoldenHex(t *testing.T) {
	usage := decodeRunAgentResponse(t, usageLimitWireHex)
	if usage.GetStatus() != "failed" || usage.GetRoundsExecuted() != 2 {
		t.Fatalf("unexpected status/rounds: %q %d", usage.GetStatus(), usage.GetRoundsExecuted())
	}
	if usage.GetFailureCode() != "subscription_usage_limit" {
		t.Fatalf("failure_code = %q", usage.GetFailureCode())
	}
	if usage.RetryAfterMs == nil || *usage.RetryAfterMs != 52260000 {
		t.Fatalf("retry_after_ms = %v", usage.RetryAfterMs)
	}
	if usage.GetResetsAt() != "2026-09-26T01:21:00Z" {
		t.Fatalf("resets_at = %q", usage.GetResetsAt())
	}

	zero := decodeRunAgentResponse(t, zeroWaitWireHex)
	if zero.RetryAfterMs == nil || *zero.RetryAfterMs != 0 {
		t.Fatalf("a reported wait of zero must be present and zero, got %v", zero.RetryAfterMs)
	}

	bare := decodeRunAgentResponse(t, noTimingWireHex)
	if bare.GetFailureCode() != "subscription_reauth_required" {
		t.Fatalf("failure_code = %q", bare.GetFailureCode())
	}
	if bare.RetryAfterMs != nil {
		t.Fatalf("an unreported wait must be absent, got %d", *bare.RetryAfterMs)
	}
	if bare.GetResetsAt() != "" {
		t.Fatalf("resets_at = %q", bare.GetResetsAt())
	}
}

func TestRunAgentResponseTypedFailureEncodesToGoldenHex(t *testing.T) {
	wait := uint64(52260000)
	zero := uint64(0)
	for _, tc := range []struct {
		name     string
		response *RunAgentResponse
		want     string
	}{
		{
			name: "usage limit with timing",
			response: &RunAgentResponse{
				Status: "failed", FinalOutput: "This run was stopped.", RoundsExecuted: 2,
				FailureCode: "subscription_usage_limit", RetryAfterMs: &wait, ResetsAt: "2026-09-26T01:21:00Z",
			},
			want: usageLimitWireHex,
		},
		{
			name: "throttle with a zero wait",
			response: &RunAgentResponse{
				Status: "failed", FinalOutput: "This run was stopped.", RoundsExecuted: 2,
				FailureCode: "rate_limited", RetryAfterMs: &zero,
			},
			want: zeroWaitWireHex,
		},
		{
			name: "refusal with no timing",
			response: &RunAgentResponse{
				Status: "failed", FinalOutput: "This run was stopped.", RoundsExecuted: 2,
				FailureCode: "subscription_reauth_required",
			},
			want: noTimingWireHex,
		},
	} {
		raw, err := proto.MarshalOptions{Deterministic: true}.Marshal(tc.response)
		if err != nil {
			t.Fatalf("%s: encode: %v", tc.name, err)
		}
		if got := hex.EncodeToString(raw); got != tc.want {
			t.Fatalf("%s: wire bytes differ\n got %s\nwant %s", tc.name, got, tc.want)
		}
	}
}

func TestRunAgentResponseWithoutTheFailureFieldsHasNoTypedFailure(t *testing.T) {
	raw, err := proto.Marshal(&RunAgentResponse{Status: "failed", FinalOutput: "x", RoundsExecuted: 1})
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	var decoded RunAgentResponse
	if err := proto.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if decoded.GetFailureCode() != "" || decoded.RetryAfterMs != nil || decoded.GetResetsAt() != "" {
		t.Fatalf("an old response must decode with the failure unset: %+v", &decoded)
	}
}

// RunAgentRequest.run_context (field 18, build plan WS02 S4). The same bytes are
// pinned in rust/crates/mp-contracts/tests/proto_wire_parity_execution.rs.
const runContextWireHex = "0a0272319201160a0942652062726965662e1a016d22060a0408011001"

func TestRunAgentRequestRunContextDecodesFromGoldenHex(t *testing.T) {
	raw, err := hex.DecodeString(runContextWireHex)
	if err != nil {
		t.Fatalf("bad golden hex: %v", err)
	}
	var decoded RunAgentRequest
	if err := proto.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	rc := decoded.GetRunContext()
	if rc == nil {
		t.Fatal("run_context (field 18) missing")
	}
	if rc.GetAuthoredInstructions() != "Be brief." || rc.GetMemoryBlock() != "m" {
		t.Fatalf("unexpected context: %q %q", rc.GetAuthoredInstructions(), rc.GetMemoryBlock())
	}
	part := rc.GetStatus().GetInstructions()
	if part.GetState() != RunContextState_RUN_CONTEXT_STATE_INCLUDED || part.GetIncluded() != 1 {
		t.Fatalf("unexpected status part: %v", part)
	}
}

func TestRunAgentRequestWithoutRunContextHasNone(t *testing.T) {
	raw, err := proto.Marshal(&RunAgentRequest{RunId: "r1"})
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	var decoded RunAgentRequest
	if err := proto.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if decoded.GetRunContext() != nil {
		t.Fatal("an absent run_context must stay absent")
	}
}

// RunAgentResponse.usage (field 9, build plan WS02 S3). The same bytes are pinned
// in rust/crates/mp-contracts/tests/proto_wire_parity_execution.rs.
const (
	usageWireHex     = "0a09636f6d706c6574656418034a2b08b00910d40218a006206428033209616e7468726f7069633a11636c617564652d736f6e6e65742d352d35"
	zeroUsageWireHex = "0a09636f6d706c6574656418034a00"
)

func TestRunAgentResponseUsageDecodesFromGoldenHex(t *testing.T) {
	usage := decodeRunAgentResponse(t, usageWireHex).GetUsage()
	if usage == nil {
		t.Fatal("usage (field 9) missing")
	}
	if usage.GetInputTokens() != 1200 || usage.GetOutputTokens() != 340 {
		t.Fatalf("unexpected tokens: %d %d", usage.GetInputTokens(), usage.GetOutputTokens())
	}
	if usage.GetCacheReadInputTokens() != 800 || usage.GetCacheCreationInputTokens() != 100 {
		t.Fatalf("unexpected cache legs: %d %d", usage.GetCacheReadInputTokens(), usage.GetCacheCreationInputTokens())
	}
	if usage.GetInferenceRounds() != 3 || usage.GetProviderUsed() != "anthropic" || usage.GetModelUsed() != "claude-sonnet-5-5" {
		t.Fatalf("unexpected rounds or route: %v", usage)
	}

	// A run that spent nothing is a present, empty message: not absence.
	if decodeRunAgentResponse(t, zeroUsageWireHex).GetUsage() == nil {
		t.Fatal("a run that spent nothing must report so, not look like an old executor")
	}
}

func TestRunAgentResponseUsageEncodesToGoldenHex(t *testing.T) {
	for _, tc := range []struct {
		name     string
		response *RunAgentResponse
		want     string
	}{
		{
			name: "three rounds with cache legs",
			response: &RunAgentResponse{
				Status: "completed", RoundsExecuted: 3,
				Usage: &RunUsage{
					InputTokens: 1200, OutputTokens: 340, CacheReadInputTokens: 800, CacheCreationInputTokens: 100,
					InferenceRounds: 3, ProviderUsed: "anthropic", ModelUsed: "claude-sonnet-5-5",
				},
			},
			want: usageWireHex,
		},
		{
			name:     "no spend",
			response: &RunAgentResponse{Status: "completed", RoundsExecuted: 3, Usage: &RunUsage{}},
			want:     zeroUsageWireHex,
		},
	} {
		raw, err := proto.MarshalOptions{Deterministic: true}.Marshal(tc.response)
		if err != nil {
			t.Fatalf("%s: encode: %v", tc.name, err)
		}
		if got := hex.EncodeToString(raw); got != tc.want {
			t.Fatalf("%s: wire bytes differ\n got %s\nwant %s", tc.name, got, tc.want)
		}
	}
}

func TestRunAgentResponseWithoutUsageHasNone(t *testing.T) {
	raw, err := proto.Marshal(&RunAgentResponse{Status: "completed"})
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	var decoded RunAgentResponse
	if err := proto.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if decoded.GetUsage() != nil {
		t.Fatal("an absent usage must stay absent")
	}
}

// RunUsage.legs (field 8, a repeated RunUsageLeg): the same spend split by the
// provider and model that served it. The same bytes are pinned in
// rust/crates/mp-contracts/tests/proto_wire_parity_execution.rs.
const usageWithLegsWireHex = "0a09636f6d706c6574656418034a7008b00910d40218a006206428033209616e7468726f7069633a11636c617564652d736f6e6e65742d352d35422b0a09616e7468726f7069631211636c617564652d736f6e6e65742d352d3518e80720ac0228a0063064380242160a066f70656e616912056770742d3518c80120283801"

func usageWithTwoLegs() *RunUsage {
	return &RunUsage{
		InputTokens: 1200, OutputTokens: 340, CacheReadInputTokens: 800, CacheCreationInputTokens: 100,
		InferenceRounds: 3, ProviderUsed: "anthropic", ModelUsed: "claude-sonnet-5-5",
		Legs: []*RunUsageLeg{
			{
				ProviderUsed: "anthropic", ModelUsed: "claude-sonnet-5-5",
				InputTokens: 1000, OutputTokens: 300, CacheReadInputTokens: 800, CacheCreationInputTokens: 100,
				InferenceRounds: 2,
			},
			{
				ProviderUsed: "openai", ModelUsed: "gpt-5",
				InputTokens: 200, OutputTokens: 40, InferenceRounds: 1,
			},
		},
	}
}

func TestRunUsageLegsEncodeToGoldenHex(t *testing.T) {
	raw, err := proto.MarshalOptions{Deterministic: true}.Marshal(
		&RunAgentResponse{Status: "completed", RoundsExecuted: 3, Usage: usageWithTwoLegs()},
	)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	if got := hex.EncodeToString(raw); got != usageWithLegsWireHex {
		t.Fatalf("wire bytes differ\n got %s\nwant %s", got, usageWithLegsWireHex)
	}
}

func TestRunUsageLegsDecodeFromGoldenHex(t *testing.T) {
	usage := decodeRunAgentResponse(t, usageWithLegsWireHex).GetUsage()
	if usage == nil {
		t.Fatal("usage (field 9) missing")
	}
	legs := usage.GetLegs()
	if len(legs) != 2 {
		t.Fatalf("want 2 legs, got %d", len(legs))
	}
	if legs[0].GetProviderUsed() != "anthropic" || legs[0].GetModelUsed() != "claude-sonnet-5-5" ||
		legs[0].GetInputTokens() != 1000 || legs[0].GetOutputTokens() != 300 ||
		legs[0].GetCacheReadInputTokens() != 800 || legs[0].GetCacheCreationInputTokens() != 100 ||
		legs[0].GetInferenceRounds() != 2 {
		t.Fatalf("unexpected first leg: %v", legs[0])
	}
	if legs[1].GetProviderUsed() != "openai" || legs[1].GetModelUsed() != "gpt-5" ||
		legs[1].GetInputTokens() != 200 || legs[1].GetOutputTokens() != 40 || legs[1].GetInferenceRounds() != 1 {
		t.Fatalf("unexpected second leg: %v", legs[1])
	}
	var sum uint64
	for _, leg := range legs {
		sum += leg.GetInputTokens()
	}
	if sum != usage.GetInputTokens() {
		t.Fatalf("legs sum to %d, totals say %d", sum, usage.GetInputTokens())
	}
	// The legs-free golden still decodes, with none: an executor without legs.
	if len(decodeRunAgentResponse(t, usageWireHex).GetUsage().GetLegs()) != 0 {
		t.Fatal("a usage without legs must decode with none")
	}
}
