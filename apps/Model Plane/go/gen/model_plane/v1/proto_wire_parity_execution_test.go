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
