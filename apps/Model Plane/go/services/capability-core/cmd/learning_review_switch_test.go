package main

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
)

func TestParseLearningReviewSwitch(t *testing.T) {
	cases := []struct {
		raw            string
		wantEnabled    bool
		wantRecognized bool
	}{
		// Unset or blank is the code default: enabled, so a deployment that never
		// heard of the switch keeps its learning loop.
		{"", true, true},
		{"   ", true, true},
		{"true", true, true},
		{"TRUE", true, true},
		{"1", true, true},
		{"yes", true, true},
		{" on ", true, true},
		{"enabled", true, true},
		{"false", false, true},
		{"FALSE", false, true},
		{" False ", false, true},
		{"0", false, true},
		{"no", false, true},
		{"off", false, true},
		// The repo documents 0/off/false/no/disabled for its other switches; a cost
		// guard must not stay on because someone wrote the word they expected to work.
		{"disabled", false, true},
		{" Disabled ", false, true},
		// A value that is neither a clear yes nor a clear no keeps the default and
		// is reported as unrecognized rather than silently guessed.
		{"flase", true, false},
		{"nope", true, false},
		{"2", true, false},
	}
	for _, tc := range cases {
		enabled, recognized := parseLearningReviewSwitch(tc.raw)
		if enabled != tc.wantEnabled || recognized != tc.wantRecognized {
			t.Errorf("parseLearningReviewSwitch(%q) = (enabled=%v, recognized=%v), want (%v, %v)",
				tc.raw, enabled, recognized, tc.wantEnabled, tc.wantRecognized)
		}
	}
}

func TestLearningReviewEnabledReadsTheEnvironment(t *testing.T) {
	t.Setenv(learningReviewEnabledEnv, "")
	if !learningReviewEnabled() {
		t.Fatal("a blank " + learningReviewEnabledEnv + " must leave the review enabled (code default)")
	}
	t.Setenv(learningReviewEnabledEnv, "false")
	if learningReviewEnabled() {
		t.Fatal(learningReviewEnabledEnv + "=false must disable the review")
	}
	t.Setenv(learningReviewEnabledEnv, "true")
	if !learningReviewEnabled() {
		t.Fatal(learningReviewEnabledEnv + "=true must enable the review")
	}
}

// forbiddenSessionClient and forbiddenInferenceClient fail the test on any RPC.
// They embed the generated interface, so any method call that the consumer makes
// would hit the nil embedded value and panic; the explicit overrides below turn
// the two calls the review actually makes into readable failures first.
type forbiddenSessionClient struct {
	mpv1.SessionCoreClient
	t *testing.T
}

func (f forbiddenSessionClient) ListConversation(
	context.Context, *mpv1.ListConversationRequest, ...grpc.CallOption,
) (*mpv1.ListConversationResponse, error) {
	f.t.Helper()
	f.t.Fatal("session-core was called although the learning review is switched off")
	return nil, nil
}

type forbiddenInferenceClient struct {
	mpv1.InferenceCoreClient
	t *testing.T
}

func (f forbiddenInferenceClient) Infer(
	context.Context, *mpv1.InferRequest, ...grpc.CallOption,
) (*mpv1.InferResponse, error) {
	f.t.Helper()
	f.t.Fatal("inference-core was called although the learning review is switched off")
	return nil, nil
}

// captureSlog routes the default slog logger into a buffer for the test.
func captureSlog(t *testing.T) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug})))
	t.Cleanup(func() { slog.SetDefault(previous) })
	return &buf
}

func TestStartLearningConsumerDoesNotSubscribeWhenSwitchedOff(t *testing.T) {
	t.Setenv(learningReviewEnabledEnv, "false")
	logs := captureSlog(t)

	// Both backends are present, which is the exact situation in which the
	// consumer used to start: the only thing that can stop it now is the switch.
	// nc is nil on purpose. A consumer that got past the switch would call
	// nc.JetStream() and dereference it, so a regression shows up as a panic here
	// instead of a quiet subscription.
	startLearningConsumer(context.Background(), nil,
		forbiddenSessionClient{t: t}, forbiddenInferenceClient{t: t})

	lines := strings.Split(strings.TrimSpace(logs.String()), "\n")
	if len(lines) != 1 || lines[0] == "" {
		t.Fatalf("a switched-off consumer must log exactly one line, got %d: %q", len(lines), logs.String())
	}
	if !strings.Contains(lines[0], "learning-review consumer disabled") {
		t.Fatalf("log line does not say the consumer is disabled: %q", lines[0])
	}
	if !strings.Contains(lines[0], learningReviewEnabledEnv) {
		t.Fatalf("log line does not name the switch that disabled it: %q", lines[0])
	}
	if strings.Contains(lines[0], "started") || strings.Contains(lines[0], "bound") {
		t.Fatalf("log line claims the consumer started: %q", lines[0])
	}
}

func TestStartLearningConsumerSwitchedOffWinsOverMissingBackends(t *testing.T) {
	// Off and "backends unset" both mean no consumer; the switch is named when
	// it is the reason, so an operator reading the boot log is not misled into
	// hunting for a missing SESSION_CORE_ADDR.
	t.Setenv(learningReviewEnabledEnv, "false")
	logs := captureSlog(t)

	startLearningConsumer(context.Background(), nil, nil, nil)

	if !strings.Contains(logs.String(), learningReviewEnabledEnv) {
		t.Fatalf("switched-off log must name %s even when the clients are nil: %q",
			learningReviewEnabledEnv, logs.String())
	}
}
