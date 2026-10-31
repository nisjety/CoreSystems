package notification

import (
	"context"
	"testing"
)

// fakeChannelGate answers the per-type question and the in-app question separately, so a test can show
// which of the two decides for a given type.
type fakeChannelGate struct {
	legacyEnabled bool
	inApp         bool
	err           error
}

func (gate fakeChannelGate) IsNotificationEnabled(context.Context, string, string, string) (bool, error) {
	return gate.legacyEnabled, gate.err
}

func (gate fakeChannelGate) IsEnabled(_ context.Context, _, _, _, channel string, fallback bool) (bool, error) {
	if gate.err != nil {
		return false, gate.err
	}
	if channel == inAppChannel {
		return gate.inApp, nil
	}
	return fallback, nil
}

// attentionRequest is the request capability-core sends for one waiting state: fixed copy and opaque ids.
func attentionRequest(notificationType string) Request {
	return Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           notificationType,
		Payload: map[string]any{
			"title":       "Approval needed",
			"body":        "A run in one of your chats is waiting for your approval.",
			"message_key": notificationType,
			"cta_href":    "/chat?thread=thread-9",
			"cta_label":   "Open chat",
			"thread_id":   "thread-9",
			"run_id":      "run-abc",
			"kind":        "needs_approval",
		},
		Source:        "capability-core",
		RetentionMode: RetentionModeStandard,
	}
}

func nativeTestService(repository Repository, runtimeClient RuntimeClient, gate PreferenceGate, opts ...Option) *Service {
	base := []Option{
		WithIDGenerator(func() string { return "req_native" }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithPreferenceGate(gate),
		WithNativeDelivery(),
	}
	return NewService(repository, runtimeClient, &fakePublisher{}, append(base, opts...)...)
}

func TestNativeTypeIsNotSuppressedByAnEmailOptOut(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{Provider: ProviderNative, ProviderRequestID: "native:req_native"}}
	service := nativeTestService(repository, runtimeClient, fakeChannelGate{legacyEnabled: false, inApp: true})

	result, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval"))
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if result.Status != StatusSubmitted {
		t.Fatalf("Status = %q, want %q: an email opt-out must not suppress the in-app row", result.Status, StatusSubmitted)
	}
	if runtimeClient.callCount != 1 {
		t.Fatalf("runtime calls = %d, want 1", runtimeClient.callCount)
	}
	if got := repository.stored["req_native"].Provider; got != ProviderNative {
		t.Fatalf("Provider = %q, want %q", got, ProviderNative)
	}
}

func TestNativeTypeIsSuppressedWhenInAppIsOff(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "must-not-send"}}
	service := nativeTestService(repository, runtimeClient, fakeChannelGate{legacyEnabled: true, inApp: false})

	result, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval"))
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if result.Status != StatusSuppressed {
		t.Fatalf("Status = %q, want %q", result.Status, StatusSuppressed)
	}
	if runtimeClient.callCount != 0 {
		t.Fatalf("runtime calls = %d, want 0", runtimeClient.callCount)
	}
	if got := repository.stored["req_native"].Provider; got != ProviderLocalPolicy {
		t.Fatalf("Provider = %q, want %q", got, ProviderLocalPolicy)
	}
}

func TestLegacyTypeKeepsItsPerTypeGate(t *testing.T) {
	// Types outside the native set still read the per-type gate, so a legacy type with an opt-out on any
	// channel stays suppressed. Changing that for every type is a decision of its own, not a side effect.
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "must-not-send"}}
	service := nativeTestService(repository, runtimeClient, fakeChannelGate{legacyEnabled: false, inApp: true})

	result, err := service.Accept(context.Background(), Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           "comment.mentioned",
	})
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if result.Status != StatusSuppressed {
		t.Fatalf("Status = %q, want %q", result.Status, StatusSuppressed)
	}
	if runtimeClient.callCount != 0 {
		t.Fatalf("runtime calls = %d, want 0", runtimeClient.callCount)
	}
}

func TestNativeTypeRefusesZeroRetentionBeforeAnyWrite(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{}
	service := nativeTestService(repository, runtimeClient, fakeChannelGate{legacyEnabled: true, inApp: true})

	request := attentionRequest("modelplane.needs_approval")
	request.RetentionMode = RetentionModeZDR
	result, err := service.Accept(context.Background(), request)
	if result != nil || !IsValidationError(err) {
		t.Fatalf("Accept() = (%#v, %v), want a validation error and no result", result, err)
	}
	if repository.createCalls != 0 || runtimeClient.callCount != 0 {
		t.Fatalf("creates = %d, runtime calls = %d, want neither", repository.createCalls, runtimeClient.callCount)
	}
}

func TestNativeTypeWithoutNativeDeliveryKeepsTheProviderPath(t *testing.T) {
	// The switch off means the service is built without WithNativeDelivery, and the type is stored under
	// the provider exactly as every other type is.
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "novu-txn-1"}}
	service := NewService(
		repository,
		runtimeClient,
		&fakePublisher{},
		WithIDGenerator(func() string { return "req_native" }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithPreferenceGate(fakeChannelGate{legacyEnabled: true, inApp: true}),
	)

	result, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval"))
	if err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if result.Status != StatusSubmitted {
		t.Fatalf("Status = %q, want %q", result.Status, StatusSubmitted)
	}
	if got := repository.stored["req_native"].Provider; got != ProviderNovu {
		t.Fatalf("Provider = %q, want %q", got, ProviderNovu)
	}
}

func TestNativeTypeGateFailsClosedWithoutChannelGate(t *testing.T) {
	service := nativeTestService(newFakeRepository(), &fakeRuntimeClient{}, fakePreferenceGate{enabled: true})

	result, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval"))
	if result != nil || err == nil {
		t.Fatalf("Accept() = (%#v, %v), want a non-nil error and no result", result, err)
	}
}

func TestNativeFeedMirrorCarriesTheNativeProvider(t *testing.T) {
	var mirrored []FeedSinkParams
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{Provider: ProviderNative, ProviderRequestID: "native:req_native"}}
	service := nativeTestService(
		newFakeRepository(),
		runtimeClient,
		fakeChannelGate{legacyEnabled: true, inApp: true},
		WithFeedSink(func(_ context.Context, params FeedSinkParams) {
			mirrored = append(mirrored, params)
		}),
	)

	if _, err := service.Accept(context.Background(), attentionRequest("modelplane.needs_approval")); err != nil {
		t.Fatalf("Accept() error = %v", err)
	}
	if len(mirrored) != 1 {
		t.Fatalf("feed mirror calls = %d, want 1", len(mirrored))
	}
	if mirrored[0].Provider != ProviderNative || mirrored[0].ProviderTransactionID != "native:req_native" {
		t.Fatalf("feed row provider = %q / %q, want native and native:req_native", mirrored[0].Provider, mirrored[0].ProviderTransactionID)
	}
	if mirrored[0].Title != "Approval needed" || mirrored[0].CtaHref != "/chat?thread=thread-9" {
		t.Fatalf("feed row title = %q, href = %q, want the fixed copy and the thread link", mirrored[0].Title, mirrored[0].CtaHref)
	}
}
