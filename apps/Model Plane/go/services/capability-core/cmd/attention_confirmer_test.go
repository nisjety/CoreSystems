package main

import (
	"context"
	"errors"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
)

// recordingAttentionClient is the AttentionServiceClient double for the confirmer. It records
// the one request it is asked and answers with the response and error it was given. Only
// ConfirmAttentionWaiting is implemented; any other method reached through the embedded nil
// interface panics, which a test of this adapter must never reach.
type recordingAttentionClient struct {
	mpv1.AttentionServiceClient
	asked    *mpv1.ConfirmAttentionWaitingRequest
	response *mpv1.ConfirmAttentionWaitingResponse
	err      error
}

func (c *recordingAttentionClient) ConfirmAttentionWaiting(_ context.Context, in *mpv1.ConfirmAttentionWaitingRequest, _ ...grpc.CallOption) (*mpv1.ConfirmAttentionWaitingResponse, error) {
	c.asked = in
	return c.response, c.err
}

// The confirmer names the item by the ids its envelope carries and the state by the enum of
// the same name. A state name outside the closed set reaches session-core as the unspecified
// state, so no unknown state is ever asked about as a waiting state.
func TestAttentionConfirmerAsksWithTheItemIdsAndTheStateEnum(t *testing.T) {
	cases := []struct {
		name string
		want mpv1.AttentionState
	}{
		{"ATTENTION_STATE_NEEDS_APPROVAL", mpv1.AttentionState_ATTENTION_STATE_NEEDS_APPROVAL},
		{"ATTENTION_STATE_NEEDS_INPUT", mpv1.AttentionState_ATTENTION_STATE_NEEDS_INPUT},
		{"ATTENTION_STATE_PLAN_READY", mpv1.AttentionState_ATTENTION_STATE_PLAN_READY},
		{"ATTENTION_STATE_COMPLETED", mpv1.AttentionState_ATTENTION_STATE_COMPLETED},
		{"ATTENTION_STATE_MADE_UP", mpv1.AttentionState_ATTENTION_STATE_UNSPECIFIED},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			client := &recordingAttentionClient{response: &mpv1.ConfirmAttentionWaitingResponse{Enabled: true, Waiting: true}}
			if _, _, err := (attentionConfirmer{client: client}).StillWaiting(context.Background(), "org-1", "user-1", "thread-9", "run-abc", "apr-1", tc.name); err != nil {
				t.Fatalf("StillWaiting: %v", err)
			}
			got := client.asked
			if got.GetOrgId() != "org-1" || got.GetUserId() != "user-1" || got.GetThreadId() != "thread-9" || got.GetRunId() != "run-abc" || got.GetSourceId() != "apr-1" {
				t.Fatalf("asked with ids %v, want the envelope's org, user, thread, run and source", got)
			}
			if got.GetState() != tc.want {
				t.Fatalf("asked with state %v, want %v", got.GetState(), tc.want)
			}
		})
	}
}

// session-core's answer is handed back as it came, so the notifier acks an item session-core
// has switched off or decided, and announces one it still holds as waiting.
func TestAttentionConfirmerReturnsSessionCoreAnswerUnchanged(t *testing.T) {
	cases := []struct {
		name             string
		enabled, waiting bool
	}{
		{"waiting while enabled", true, true},
		{"decided while enabled", true, false},
		{"switched off", false, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			client := &recordingAttentionClient{response: &mpv1.ConfirmAttentionWaitingResponse{Enabled: tc.enabled, Waiting: tc.waiting}}
			enabled, waiting, err := (attentionConfirmer{client: client}).StillWaiting(context.Background(), "org-1", "user-1", "thread-9", "run-abc", "apr-1", "ATTENTION_STATE_NEEDS_APPROVAL")
			if err != nil {
				t.Fatalf("StillWaiting: %v", err)
			}
			if enabled != tc.enabled || waiting != tc.waiting {
				t.Fatalf("StillWaiting = (%v, %v), want (%v, %v)", enabled, waiting, tc.enabled, tc.waiting)
			}
		})
	}
}

// A failed ask is an error with neither answer, so the notifier retries the item and never
// announces it unconfirmed.
func TestAttentionConfirmerReportsAFailedAskAsAnError(t *testing.T) {
	client := &recordingAttentionClient{err: errors.New("session-core unreachable")}
	enabled, waiting, err := (attentionConfirmer{client: client}).StillWaiting(context.Background(), "org-1", "user-1", "thread-9", "run-abc", "apr-1", "ATTENTION_STATE_NEEDS_APPROVAL")
	if err == nil {
		t.Fatal("StillWaiting returned no error for a failed ask")
	}
	if enabled || waiting {
		t.Fatalf("StillWaiting = (%v, %v) on a failed ask, want (false, false)", enabled, waiting)
	}
}
