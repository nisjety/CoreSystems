package notification

import (
	"context"
	"errors"
	"testing"
)

type fakePreferenceGate struct {
	enabled bool
	err     error
}

func (gate fakePreferenceGate) IsNotificationEnabled(context.Context, string, string, string) (bool, error) {
	return gate.enabled, gate.err
}

func TestAcceptSuppressesLocallyOptedOutRecipientBeforeDispatch(t *testing.T) {
	repository := newFakeRepository()
	runtimeClient := &fakeRuntimeClient{result: &DispatchResult{ProviderRequestID: "must-not-send"}}
	service := NewService(
		repository,
		runtimeClient,
		&fakePublisher{},
		WithIDGenerator(func() string { return "req_suppressed" }),
		WithRecipientResolver(fakeRecipientResolver{}),
		WithPreferenceGate(fakePreferenceGate{enabled: false}),
	)

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

func TestAcceptFailsClosedWhenPreferenceLookupFails(t *testing.T) {
	service := NewService(
		newFakeRepository(),
		&fakeRuntimeClient{},
		&fakePublisher{},
		WithRecipientResolver(fakeRecipientResolver{}),
		WithPreferenceGate(fakePreferenceGate{err: errors.New("preference database unavailable")}),
	)

	result, err := service.Accept(context.Background(), Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           "comment.mentioned",
	})
	if result != nil || err == nil {
		t.Fatalf("Accept() = (%#v, %v), want nil error result and non-nil error", result, err)
	}
}
