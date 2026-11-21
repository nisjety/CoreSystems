package notification

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"
)

// legacyPushGate answers only the plain question, so it cannot answer the per-channel one the push needs.
type legacyPushGate struct{}

func (legacyPushGate) IsNotificationEnabled(context.Context, string, string, string) (bool, error) {
	return true, nil
}

// pushWorkerRequest is a standard notice for one thread, the shape a queued attention notice takes.
func pushWorkerRequest(notificationType string) *StoredRequest {
	return &StoredRequest{
		ID:             "req-1",
		OrganizationID: "org-1",
		RecipientKind:  RecipientKindUser,
		RecipientID:    "user-1",
		Type:           notificationType,
		Payload:        map[string]any{"title": "Approval needed", "thread_id": "thread-9"},
		RetentionMode:  RetentionModeStandard,
	}
}

// scheduledWorkerRequest is a scheduled-run outcome for the same thread.
func scheduledWorkerRequest(outcome string) *StoredRequest {
	request := pushWorkerRequest(scheduledRunOutcomeType)
	request.Payload = map[string]any{"outcome": outcome, "thread_id": "thread-9"}
	return request
}

// runPushWorker runs one attempt of the request through a queued worker built with the given dispatcher. A nil
// dispatcher builds the worker without one. It returns the queue the worker wrote its outcome to.
func runPushWorker(
	t *testing.T,
	request *StoredRequest,
	runtime *workerRuntimeFake,
	resolver RecipientResolver,
	dispatcher *pushDispatcher,
) *workerQueueFake {
	t.Helper()
	queue := &workerQueueFake{attempt: &DeliveryAttempt{ID: "attempt-1", NotificationID: "req-1", Status: DeliveryAttemptClaimed}}
	repository := &workerRequestRepositoryFake{request: request}
	worker := NewDeliveryWorker(repository, queue, runtime, resolver,
		WithDeliveryWorkerClock(func() time.Time { return time.Unix(100, 0).UTC() }),
		WithDeliveryWorkerPush(dispatcher),
	)
	if _, err := worker.RunOnce(context.Background(), "worker-1"); err != nil {
		t.Fatalf("RunOnce() error = %v", err)
	}
	return queue
}

// A notice the queued worker submits wakes its recipient once, with the thread and the recipient only.
func TestDeliveryWorkerPushesASubmittedNativeNoticeOnce(t *testing.T) {
	notifier := &recordingPushNotifier{}
	dispatcher := &pushDispatcher{notifier: notifier, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}
	runtime := &workerRuntimeFake{result: &DispatchResult{Provider: ProviderNative, ProviderRequestID: "native:req-1"}}

	queue := runPushWorker(t, pushWorkerRequest("modelplane.needs_approval"), runtime, fakeRecipientResolver{}, dispatcher)

	if queue.submitted != 1 || queue.unknown != 0 || queue.failed != 0 {
		t.Fatalf("submitted=%d unknown=%d failed=%d, want one submission", queue.submitted, queue.unknown, queue.failed)
	}
	want := []PushNotice{{OrganizationID: "org-1", RecipientID: "user-1", Type: "modelplane.needs_approval", ThreadID: "thread-9"}}
	if !reflect.DeepEqual(notifier.notices, want) {
		t.Fatalf("push notices = %#v, want %#v", notifier.notices, want)
	}
}

// A notice that is not submitted never wakes anyone: a dispatch error, a missing provider id, a zero-retention notice
// refused before dispatch, and a notice whose recipient cannot be resolved.
func TestDeliveryWorkerDoesNotPushUnknownOrFailedNotices(t *testing.T) {
	zeroRetention := pushWorkerRequest("modelplane.needs_approval")
	zeroRetention.RetentionMode = RetentionModeZDR
	cases := []struct {
		name     string
		request  *StoredRequest
		runtime  *workerRuntimeFake
		resolver RecipientResolver
	}{
		{name: "provider dispatch error", request: pushWorkerRequest("modelplane.needs_approval"),
			runtime: &workerRuntimeFake{err: errors.New("provider timeout")}, resolver: fakeRecipientResolver{}},
		{name: "provider request id missing", request: pushWorkerRequest("modelplane.needs_approval"),
			runtime: &workerRuntimeFake{result: &DispatchResult{}}, resolver: fakeRecipientResolver{}},
		{name: "zero-retention notice", request: zeroRetention,
			runtime: &workerRuntimeFake{result: &DispatchResult{ProviderRequestID: "native:req-1"}}, resolver: fakeRecipientResolver{}},
		{name: "recipient resolver unavailable", request: pushWorkerRequest("modelplane.needs_approval"),
			runtime: &workerRuntimeFake{result: &DispatchResult{ProviderRequestID: "native:req-1"}}, resolver: nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			notifier := &recordingPushNotifier{}
			dispatcher := &pushDispatcher{notifier: notifier, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}

			queue := runPushWorker(t, tc.request, tc.runtime, tc.resolver, dispatcher)

			if queue.submitted != 0 {
				t.Fatalf("submitted = %d, want no submission", queue.submitted)
			}
			if len(notifier.notices) != 0 {
				t.Fatalf("push notices = %#v, want none for a notice that was not submitted", notifier.notices)
			}
		})
	}
}

// The worker applies the synchronous path's guards: a switched-off push channel, a scheduled outcome that is not a
// waiting one, a type that is not native, native delivery off, and a gate that cannot answer per channel.
func TestDeliveryWorkerPushFollowsTheSynchronousGuards(t *testing.T) {
	cases := []struct {
		name       string
		request    *StoredRequest
		dispatcher func(notifier PushNotifier) *pushDispatcher
		wantPush   bool
	}{
		{
			name:    "needs approval pushes",
			request: pushWorkerRequest("modelplane.needs_approval"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}
			},
			wantPush: true,
		},
		{
			name:    "push channel off",
			request: pushWorkerRequest("modelplane.needs_approval"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: pushGate{inApp: true, push: false}, nativeDelivery: true}
			},
		},
		{
			name:    "native delivery off",
			request: pushWorkerRequest("modelplane.needs_approval"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: false}
			},
		},
		{
			name:    "a provider type never pushes",
			request: pushWorkerRequest("comment.mentioned"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}
			},
		},
		{
			name:    "scheduled run needs review pushes",
			request: scheduledWorkerRequest("needs_review"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}
			},
			wantPush: true,
		},
		{
			name:    "scheduled run completed stays in the feed",
			request: scheduledWorkerRequest("completed"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}
			},
		},
		{
			name:    "a gate without the channel form never pushes",
			request: pushWorkerRequest("modelplane.needs_approval"),
			dispatcher: func(n PushNotifier) *pushDispatcher {
				return &pushDispatcher{notifier: n, preferenceGate: legacyPushGate{}, nativeDelivery: true}
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			notifier := &recordingPushNotifier{}
			runtime := &workerRuntimeFake{result: &DispatchResult{ProviderRequestID: "native:req-1"}}

			queue := runPushWorker(t, tc.request, runtime, fakeRecipientResolver{}, tc.dispatcher(notifier))

			if queue.submitted != 1 {
				t.Fatalf("submitted = %d, want the notice submitted whatever the push decides", queue.submitted)
			}
			if got := len(notifier.notices) == 1; got != tc.wantPush {
				t.Fatalf("pushed = %v, want %v (notices %#v)", got, tc.wantPush, notifier.notices)
			}
		})
	}
}

// A worker built without a dispatcher submits the notice and pushes nothing, and it does not fail.
func TestDeliveryWorkerWithoutADispatcherSubmitsWithoutPushing(t *testing.T) {
	runtime := &workerRuntimeFake{result: &DispatchResult{ProviderRequestID: "native:req-1"}}

	queue := runPushWorker(t, pushWorkerRequest("modelplane.needs_approval"), runtime, fakeRecipientResolver{}, nil)

	if queue.submitted != 1 {
		t.Fatalf("submitted = %d, want one submission without a push dispatcher", queue.submitted)
	}
}

// A dispatcher with no notifier, or a nil dispatcher, pushes nothing and does not panic.
func TestPushDispatcherWithoutANotifierOrANilDispatcherPushesNothing(t *testing.T) {
	request := pushWorkerRequest("modelplane.needs_approval")
	(&pushDispatcher{preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}).dispatch(context.Background(), request)

	var nilDispatcher *pushDispatcher
	nilDispatcher.dispatch(context.Background(), request)
	nilDispatcher.dispatch(context.Background(), nil)
}

// The queued worker pushes with the service's own dispatcher: the same notifier, the same gate, the same native switch.
func TestServicePushDispatcherSharesTheNotifierGateAndNativeSwitch(t *testing.T) {
	notifier := &recordingPushNotifier{}
	gate := pushGate{inApp: true, push: true}
	service := nativeTestService(newFakeRepository(), &fakeRuntimeClient{}, gate, WithPushNotifier(notifier))

	dispatcher := service.PushDispatcher()

	if dispatcher == nil || dispatcher.notifier != PushNotifier(notifier) || dispatcher.preferenceGate != PreferenceGate(gate) || !dispatcher.nativeDelivery {
		t.Fatalf("dispatcher = %#v, want the service's notifier, gate and native switch", dispatcher)
	}
}

// failingSubmitQueue refuses the one write that makes a notice submitted, so the worker reports the failure and pushes
// nothing.
type failingSubmitQueue struct {
	*workerQueueFake
}

func (failingSubmitQueue) MarkDeliverySubmitted(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error) {
	return nil, errors.New("delivery store unavailable")
}

// A notice whose submit write fails is not submitted, so no wake-up follows the failed write.
func TestDeliveryWorkerDoesNotPushWhenTheSubmitWriteFails(t *testing.T) {
	notifier := &recordingPushNotifier{}
	dispatcher := &pushDispatcher{notifier: notifier, preferenceGate: pushGate{inApp: true, push: true}, nativeDelivery: true}
	queue := &workerQueueFake{attempt: &DeliveryAttempt{ID: "attempt-1", NotificationID: "req-1", Status: DeliveryAttemptClaimed}}
	repository := &workerRequestRepositoryFake{request: pushWorkerRequest("modelplane.needs_approval")}
	runtime := &workerRuntimeFake{result: &DispatchResult{Provider: ProviderNative, ProviderRequestID: "native:req-1"}}
	worker := NewDeliveryWorker(repository, failingSubmitQueue{queue}, runtime, fakeRecipientResolver{},
		WithDeliveryWorkerClock(func() time.Time { return time.Unix(100, 0).UTC() }),
		WithDeliveryWorkerPush(dispatcher),
	)

	if _, err := worker.RunOnce(context.Background(), "worker-1"); err == nil {
		t.Fatal("RunOnce() error = nil, want the failed submit write reported")
	}
	if len(notifier.notices) != 0 {
		t.Fatalf("push notices = %#v, want none after a failed submit write", notifier.notices)
	}
}
