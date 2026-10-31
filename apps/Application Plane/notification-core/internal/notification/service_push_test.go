package notification

import (
	"context"
	"reflect"
	"testing"
)

// recordingPushNotifier records every notice the service hands to the Web Push hook.
type recordingPushNotifier struct {
	notices []PushNotice
}

func (r *recordingPushNotifier) NotifyPush(_ context.Context, notice PushNotice) {
	r.notices = append(r.notices, notice)
}

// pushGate answers the in-app and push questions separately, so a test can switch either channel off.
type pushGate struct {
	inApp bool
	push  bool
}

func (gate pushGate) IsNotificationEnabled(context.Context, string, string, string) (bool, error) {
	return true, nil
}

func (gate pushGate) IsEnabled(_ context.Context, _, _, _, channel string, fallback bool) (bool, error) {
	switch channel {
	case inAppChannel:
		return gate.inApp, nil
	case pushChannel:
		return gate.push, nil
	}
	return fallback, nil
}

// scheduledOutcomeRequest is the notice capability-core sends when a scheduled run reaches an outcome.
func scheduledOutcomeRequest(outcome string) Request {
	return Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           scheduledRunOutcomeType,
		Payload: map[string]any{
			"title":       "Scheduled run needs review",
			"body":        "A scheduled run in one of your chats needs your review.",
			"message_key": "modelplane.scheduled_run_outcome." + outcome,
			"cta_href":    "/chat?thread=thread-9",
			"thread_id":   "thread-9",
			"run_id":      "run-abc",
			"outcome":     outcome,
		},
		Source:        "capability-core",
		RetentionMode: RetentionModeStandard,
	}
}

func TestSubmittedAttentionNoticePushesToTheRecipientWithItsThread(t *testing.T) {
	notifier := &recordingPushNotifier{}
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{Provider: ProviderNative, ProviderRequestID: "native:req_native"}}
	service := nativeTestService(newFakeRepository(), runtimeClient, pushGate{inApp: true, push: true}, WithPushNotifier(notifier))

	if _, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval")); err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	want := []PushNotice{{OrganizationID: "org-1", RecipientID: "user-1", Type: "modelplane.needs_approval", ThreadID: "thread-9"}}
	if !reflect.DeepEqual(notifier.notices, want) {
		t.Fatalf("push notices = %#v, want %#v", notifier.notices, want)
	}
}

func TestPushFollowsTheWaitingStatesAndTheChannelSwitches(t *testing.T) {
	providerRequest := Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           "comment.mentioned",
	}
	cases := []struct {
		name     string
		request  Request
		gate     pushGate
		wantPush bool
	}{
		{name: "needs approval pushes", request: attentionRequest("modelplane.needs_approval"), gate: pushGate{inApp: true, push: true}, wantPush: true},
		{name: "needs input pushes", request: attentionRequest("modelplane.needs_input"), gate: pushGate{inApp: true, push: true}, wantPush: true},
		{name: "plan ready pushes", request: attentionRequest("modelplane.plan_ready"), gate: pushGate{inApp: true, push: true}, wantPush: true},
		{name: "push channel off", request: attentionRequest("modelplane.needs_approval"), gate: pushGate{inApp: true, push: false}},
		{name: "in-app off suppresses the request, so nothing pushes", request: attentionRequest("modelplane.needs_approval"), gate: pushGate{inApp: false, push: true}},
		{name: "scheduled needs review pushes", request: scheduledOutcomeRequest("needs_review"), gate: pushGate{inApp: true, push: true}, wantPush: true},
		{name: "scheduled paused expired pushes", request: scheduledOutcomeRequest("paused_expired"), gate: pushGate{inApp: true, push: true}, wantPush: true},
		{name: "scheduled paused failures pushes", request: scheduledOutcomeRequest("paused_failures"), gate: pushGate{inApp: true, push: true}, wantPush: true},
		{name: "scheduled completed stays in the feed", request: scheduledOutcomeRequest("completed"), gate: pushGate{inApp: true, push: true}},
		{name: "scheduled failed stays in the feed", request: scheduledOutcomeRequest("failed"), gate: pushGate{inApp: true, push: true}},
		{name: "a provider type never pushes", request: providerRequest, gate: pushGate{inApp: true, push: true}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			notifier := &recordingPushNotifier{}
			runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "req_test"}}
			service := nativeTestService(newFakeRepository(), runtimeClient, tc.gate, WithPushNotifier(notifier))

			if _, err := service.Accept(context.Background(), tc.request); err != nil {
				t.Fatalf("Accept() error = %v", err)
			}
			if got := len(notifier.notices) == 1; got != tc.wantPush {
				t.Fatalf("pushed = %v, want %v (notices %#v)", got, tc.wantPush, notifier.notices)
			}
		})
	}
}

func TestPushNeverFollowsAZeroRetentionRequest(t *testing.T) {
	notifier := &recordingPushNotifier{}
	service := nativeTestService(newFakeRepository(), &fakeRuntimeClient{}, pushGate{inApp: true, push: true}, WithPushNotifier(notifier))

	service.notifyPush(context.Background(), &StoredRequest{
		ID:             "req_zdr",
		OrganizationID: "org-1",
		RecipientID:    "user-1",
		Type:           "modelplane.needs_approval",
		RetentionMode:  RetentionModeZDR,
		Payload:        map[string]any{"thread_id": "thread-9"},
	})
	if len(notifier.notices) != 0 {
		t.Fatalf("push notices = %#v, want none for a zero-retention request", notifier.notices)
	}
}

func TestPushRequiresTheNativeAdapter(t *testing.T) {
	notifier := &recordingPushNotifier{}
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "req_test"}}
	service := NewService(newFakeRepository(), runtimeClient, &fakePublisher{},
		WithIDGenerator(func() string { return "req_provider" }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithPreferenceGate(pushGate{inApp: true, push: true}),
		WithPushNotifier(notifier),
	)

	if _, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval")); err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if len(notifier.notices) != 0 {
		t.Fatalf("push notices = %#v, want none without the native adapter", notifier.notices)
	}
}
