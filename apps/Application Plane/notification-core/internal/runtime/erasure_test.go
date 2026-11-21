package runtime

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"testing"
)

// newErasureAdapter returns a Novu adapter whose SDK calls go to a local test server, so the erasure calls run through
// the real SDK without any network access. The handler must answer 2xx or 4xx: a 5xx would make the SDK retry with backoff.
func newErasureAdapter(t *testing.T, handler http.HandlerFunc) *NovuAdapter {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	adapter, err := NewNovuAdapter(Config{Mode: DeliveryModeNovu, SecretKey: "novu-fixture-key", BaseURL: server.URL})
	if err != nil {
		t.Fatalf("NewNovuAdapter() error = %v", err)
	}
	return adapter
}

func writeJSON(t *testing.T, w http.ResponseWriter, status int, body any) {
	t.Helper()
	encoded, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshal response: %v", err)
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write(encoded)
}

// Without a provider the erasure cannot reach Novu, so every call refuses and makes no request.
func TestErasureRefusesWithoutAProvider(t *testing.T) {
	adapter, err := NewNovuAdapter(Config{Mode: DeliveryModeDisabled})
	if err != nil {
		t.Fatalf("NewNovuAdapter() error = %v", err)
	}
	ctx := context.Background()
	if removed, err := adapter.DeleteSubscriber(ctx, "verevon:abc"); removed || !errors.Is(err, ErrDeliveryDisabled) {
		t.Fatalf("DeleteSubscriber() = (%v, %v), want ErrDeliveryDisabled", removed, err)
	}
	if deleted, err := adapter.DeleteSubscriberMessages(ctx, "verevon:abc"); deleted != 0 || !errors.Is(err, ErrDeliveryDisabled) {
		t.Fatalf("DeleteSubscriberMessages() = (%d, %v), want ErrDeliveryDisabled", deleted, err)
	}
}

// A blank subscriber id is refused before any request. Sent as-is it would address the collection, not one subscriber.
func TestErasureRefusesABlankSubscriberIdBeforeAnyRequest(t *testing.T) {
	var mu sync.Mutex
	var requests []string
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		requests = append(requests, r.Method+" "+r.URL.Path)
		w.WriteHeader(http.StatusTeapot)
	})
	ctx := context.Background()
	if _, err := adapter.DeleteSubscriber(ctx, "   "); err == nil {
		t.Fatal("DeleteSubscriber(blank) error = nil, want refusal")
	}
	if _, err := adapter.DeleteSubscriberMessages(ctx, ""); err == nil {
		t.Fatal("DeleteSubscriberMessages(blank) error = nil, want refusal")
	}
	mu.Lock()
	defer mu.Unlock()
	if len(requests) != 0 {
		t.Fatalf("blank subscriber id made requests %v, want none", requests)
	}
}

// The subscriber is removed on Novu's subscriber route, and a successful call reports the removal.
func TestDeleteSubscriberCallsTheSubscriberRouteAndReportsRemoval(t *testing.T) {
	const subscriber = "verevon:0123abcd"
	var mu sync.Mutex
	var requests []string
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		requests = append(requests, r.Method+" "+r.URL.Path)
		if r.Method != http.MethodDelete || r.URL.Path != "/v2/subscribers/"+subscriber {
			w.WriteHeader(http.StatusTeapot)
			return
		}
		writeJSON(t, w, http.StatusOK, map[string]any{"acknowledged": true, "status": "deleted"})
	})

	removed, err := adapter.DeleteSubscriber(context.Background(), subscriber)
	if err != nil || !removed {
		t.Fatalf("DeleteSubscriber() = (%v, %v), want (true, nil)", removed, err)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(requests) != 1 || requests[0] != "DELETE /v2/subscribers/"+subscriber {
		t.Fatalf("requests = %v, want one DELETE on the subscriber route", requests)
	}
}

// Novu's own 404 means the subscriber is already gone: the erasure treats it as done, so a replay succeeds.
func TestDeleteSubscriberTreatsNovusNotFoundAsAlreadyRemoved(t *testing.T) {
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		writeJSON(t, w, http.StatusNotFound, map[string]any{"statusCode": 404, "message": "Subscriber not found", "error": "Not Found"})
	})
	removed, err := adapter.DeleteSubscriber(context.Background(), "verevon:gone")
	if err != nil || removed {
		t.Fatalf("DeleteSubscriber() = (%v, %v), want (false, nil) for an already-removed subscriber", removed, err)
	}
}

// A 404 without Novu's JSON body, such as one from a misrouted base URL, is not success. It returns an error, so the
// erasure retries instead of purging local rows while a provider record may still exist.
func TestDeleteSubscriberDoesNotTreatAForeignNotFoundAsRemoved(t *testing.T) {
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html")
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte("<html>not found</html>"))
	})
	removed, err := adapter.DeleteSubscriber(context.Background(), "verevon:unknown-route")
	if err == nil || removed {
		t.Fatalf("DeleteSubscriber() = (%v, %v), want an error for a non-Novu 404", removed, err)
	}
}

// Messages are listed for one subscriber, page by page, and each is deleted. A listing larger than one page takes a
// second pass, and every delete is counted once.
func TestDeleteSubscriberMessagesDeletesEveryPageAndCountsEachDelete(t *testing.T) {
	const subscriber = "verevon:feedfacecafe"
	const internalID = "novu-internal-feedface"
	remaining := make([]string, 0, 150)
	for i := 0; i < 150; i++ {
		remaining = append(remaining, fmt.Sprintf("msg-%03d", i))
	}

	var mu sync.Mutex
	var listQueries []url.Values
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/v2/subscribers/"+subscriber:
			writeJSON(t, w, http.StatusOK, map[string]any{"_id": internalID, "subscriberId": subscriber})
		case r.Method == http.MethodGet && r.URL.Path == "/v1/messages":
			listQueries = append(listQueries, r.URL.Query())
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			n := min(limit, len(remaining))
			data := make([]map[string]any, 0, n)
			for _, id := range remaining[:n] {
				data = append(data, map[string]any{
					"_id":             id,
					"_environmentId":  "env-1",
					"_organizationId": "org-a",
					"_notificationId": "notification-" + id,
					"_subscriberId":   internalID,
					"createdAt":       "2026-01-01T00:00:00Z",
					"transactionId":   "tx-" + id,
					"channel":         "in_app",
				})
			}
			writeJSON(t, w, http.StatusOK, map[string]any{"hasMore": len(remaining) > n, "data": data, "pageSize": limit, "page": 0})
		case r.Method == http.MethodDelete && strings.HasPrefix(r.URL.Path, "/v1/messages/"):
			id := strings.TrimPrefix(r.URL.Path, "/v1/messages/")
			kept := remaining[:0]
			found := false
			for _, existing := range remaining {
				if existing == id {
					found = true
					continue
				}
				kept = append(kept, existing)
			}
			remaining = kept
			if !found {
				writeJSON(t, w, http.StatusNotFound, map[string]any{"statusCode": 404, "message": "Message not found", "error": "Not Found"})
				return
			}
			writeJSON(t, w, http.StatusOK, map[string]any{"acknowledged": true, "status": "deleted"})
		default:
			w.WriteHeader(http.StatusTeapot)
		}
	})

	deleted, err := adapter.DeleteSubscriberMessages(context.Background(), subscriber)
	if err != nil {
		t.Fatalf("DeleteSubscriberMessages() error = %v", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if deleted != 150 {
		t.Fatalf("deleted = %d, want all 150 messages", deleted)
	}
	if len(remaining) != 0 {
		t.Fatalf("%d messages were left at the provider", len(remaining))
	}
	if len(listQueries) != 2 {
		t.Fatalf("listing passes = %d, want 2 for 150 messages at page size 100", len(listQueries))
	}
	for _, query := range listQueries {
		if query.Get("subscriberId") != subscriber || query.Get("page") != "0" || query.Get("limit") != strconv.Itoa(erasureMessagePageSize) {
			t.Fatalf("listing query = %v, want subscriberId, page 0 and the erasure page size", query)
		}
	}
}

// A subscriber with no messages, or one Novu no longer knows, deletes zero messages and is not an error.
func TestDeleteSubscriberMessagesTreatsAnEmptyOrMissingSubscriberAsDone(t *testing.T) {
	empty := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/v2/subscribers/verevon:empty" {
			writeJSON(t, w, http.StatusOK, map[string]any{"_id": "novu-internal-empty", "subscriberId": "verevon:empty"})
			return
		}
		writeJSON(t, w, http.StatusOK, map[string]any{"hasMore": false, "data": []any{}, "pageSize": 100, "page": 0})
	})
	if deleted, err := empty.DeleteSubscriberMessages(context.Background(), "verevon:empty"); deleted != 0 || err != nil {
		t.Fatalf("empty listing = (%d, %v), want (0, nil)", deleted, err)
	}

	missing := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		writeJSON(t, w, http.StatusNotFound, map[string]any{"statusCode": 404, "message": "Subscriber not found", "error": "Not Found"})
	})
	if deleted, err := missing.DeleteSubscriberMessages(context.Background(), "verevon:missing"); deleted != 0 || err != nil {
		t.Fatalf("missing subscriber = (%d, %v), want (0, nil)", deleted, err)
	}
}

// A refused message delete is an error, so the erasure retries the subscriber rather than removing it with messages left.
func TestDeleteSubscriberMessagesReturnsARefusedDelete(t *testing.T) {
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/v2/subscribers/verevon:x" {
			writeJSON(t, w, http.StatusOK, map[string]any{"_id": "novu-internal-x", "subscriberId": "verevon:x"})
			return
		}
		if r.Method == http.MethodGet {
			writeJSON(t, w, http.StatusOK, map[string]any{"hasMore": false, "data": []any{
				map[string]any{"_id": "msg-1", "_environmentId": "env-1", "_organizationId": "org-a", "_notificationId": "n", "_subscriberId": "novu-internal-x", "createdAt": "2026-01-01T00:00:00Z", "transactionId": "tx", "channel": "in_app"},
			}, "pageSize": 100, "page": 0})
			return
		}
		writeJSON(t, w, http.StatusForbidden, map[string]any{"statusCode": 403, "message": "Forbidden", "error": "Forbidden"})
	})
	deleted, err := adapter.DeleteSubscriberMessages(context.Background(), "verevon:x")
	if err == nil {
		t.Fatal("DeleteSubscriberMessages() error = nil, want the refused delete")
	}
	if deleted != 0 {
		t.Fatalf("deleted = %d, want 0 when the only delete was refused", deleted)
	}
}

// erasureMessage returns one message as Novu lists it for a subscriber. Its _subscriberId is the subscriber's internal
// Novu id, which is what the erasure checks every listed message against.
func erasureMessage(id, internalSubscriberID string) map[string]any {
	return map[string]any{
		"_id":             id,
		"_environmentId":  "env-1",
		"_organizationId": "org-a",
		"_notificationId": "notification-" + id,
		"_subscriberId":   internalSubscriberID,
		"createdAt":       "2026-01-01T00:00:00Z",
		"transactionId":   "tx-" + id,
		"channel":         "in_app",
	}
}

// The listing is trusted only when every message belongs to the resolved subscriber. Here the subscriber filter is ignored
// and the page holds another subscriber's message next to ours. Nothing on that page is deleted, not even our own message.
func TestDeleteSubscriberMessagesDeletesNothingOnAPageWithAnotherSubscribersMessage(t *testing.T) {
	const subscriber = "verevon:mine"
	const internalID = "novu-internal-mine"
	var mu sync.Mutex
	var deletes []string
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/v2/subscribers/"+subscriber:
			writeJSON(t, w, http.StatusOK, map[string]any{"_id": internalID, "subscriberId": subscriber})
		case r.Method == http.MethodGet && r.URL.Path == "/v1/messages":
			writeJSON(t, w, http.StatusOK, map[string]any{"hasMore": false, "data": []any{
				erasureMessage("msg-mine", internalID),
				erasureMessage("msg-other", "novu-internal-other"),
			}, "pageSize": 100, "page": 0})
		case r.Method == http.MethodDelete:
			deletes = append(deletes, r.URL.Path)
			writeJSON(t, w, http.StatusOK, map[string]any{"acknowledged": true, "status": "deleted"})
		default:
			w.WriteHeader(http.StatusTeapot)
		}
	})

	deleted, err := adapter.DeleteSubscriberMessages(context.Background(), subscriber)
	if err == nil {
		t.Fatal("DeleteSubscriberMessages() error = nil, want a refusal for a page with another subscriber's message")
	}
	if deleted != 0 {
		t.Fatalf("deleted = %d, want 0 when the page is refused", deleted)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(deletes) != 0 {
		t.Fatalf("DELETE requests = %v, want none", deletes)
	}
}

// A message whose own subscriber names another external subscriberId is refused too, even when its internal id matches.
func TestDeleteSubscriberMessagesRefusesAMessageWhoseSubscriberIsAnother(t *testing.T) {
	const subscriber = "verevon:mine"
	const internalID = "novu-internal-mine"
	var mu sync.Mutex
	deletes := 0
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/v2/subscribers/"+subscriber:
			writeJSON(t, w, http.StatusOK, map[string]any{"_id": internalID, "subscriberId": subscriber})
		case r.Method == http.MethodGet && r.URL.Path == "/v1/messages":
			message := erasureMessage("msg-1", internalID)
			message["subscriber"] = map[string]any{"_id": internalID, "subscriberId": "verevon:other"}
			writeJSON(t, w, http.StatusOK, map[string]any{"hasMore": false, "data": []any{message}, "pageSize": 100, "page": 0})
		case r.Method == http.MethodDelete:
			deletes++
			writeJSON(t, w, http.StatusOK, map[string]any{"acknowledged": true, "status": "deleted"})
		default:
			w.WriteHeader(http.StatusTeapot)
		}
	})

	deleted, err := adapter.DeleteSubscriberMessages(context.Background(), subscriber)
	if err == nil || deleted != 0 {
		t.Fatalf("DeleteSubscriberMessages() = (%d, %v), want (0, error) for a message whose subscriber is another", deleted, err)
	}
	mu.Lock()
	defer mu.Unlock()
	if deletes != 0 {
		t.Fatalf("DELETE requests = %d, want none", deletes)
	}
}

// A subscriber Novu no longer knows has no messages left. The call ends at the lookup, with no listing and no delete.
func TestDeleteSubscriberMessagesSkipsTheListingForAnUnknownSubscriber(t *testing.T) {
	const subscriber = "verevon:gone"
	var mu sync.Mutex
	var requests []string
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		requests = append(requests, r.Method+" "+r.URL.Path)
		if r.Method == http.MethodGet && r.URL.Path == "/v2/subscribers/"+subscriber {
			writeJSON(t, w, http.StatusNotFound, map[string]any{"statusCode": 404, "message": "Subscriber not found", "error": "Not Found"})
			return
		}
		w.WriteHeader(http.StatusTeapot)
	})

	deleted, err := adapter.DeleteSubscriberMessages(context.Background(), subscriber)
	if deleted != 0 || err != nil {
		t.Fatalf("DeleteSubscriberMessages() = (%d, %v), want (0, nil) for an unknown subscriber", deleted, err)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(requests) != 1 || requests[0] != "GET /v2/subscribers/"+subscriber {
		t.Fatalf("requests = %v, want only the subscriber lookup", requests)
	}
}

// Any other failure to resolve the subscriber is returned, and nothing is listed or deleted.
func TestDeleteSubscriberMessagesReturnsAnUnresolvedSubscriberFailure(t *testing.T) {
	const subscriber = "verevon:forbidden"
	var mu sync.Mutex
	var requests []string
	adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		requests = append(requests, r.Method+" "+r.URL.Path)
		writeJSON(t, w, http.StatusForbidden, map[string]any{"statusCode": 403, "message": "Forbidden", "error": "Forbidden"})
	})

	deleted, err := adapter.DeleteSubscriberMessages(context.Background(), subscriber)
	if deleted != 0 || err == nil {
		t.Fatalf("DeleteSubscriberMessages() = (%d, %v), want (0, error) when the subscriber lookup is refused", deleted, err)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(requests) != 1 || requests[0] != "GET /v2/subscribers/"+subscriber {
		t.Fatalf("requests = %v, want only the refused subscriber lookup", requests)
	}
}

// A subscriber answer without Novu's internal id is refused before any listing, since its messages could not be scoped.
func TestDeleteSubscriberMessagesRefusesASubscriberWithoutAnInternalID(t *testing.T) {
	const subscriber = "verevon:noid"
	answers := []map[string]any{
		{"subscriberId": subscriber},
		{"_id": "  ", "subscriberId": subscriber},
	}
	for _, answer := range answers {
		var mu sync.Mutex
		var requests []string
		adapter := newErasureAdapter(t, func(w http.ResponseWriter, r *http.Request) {
			mu.Lock()
			defer mu.Unlock()
			requests = append(requests, r.Method+" "+r.URL.Path)
			if r.Method == http.MethodGet && r.URL.Path == "/v2/subscribers/"+subscriber {
				writeJSON(t, w, http.StatusOK, answer)
				return
			}
			w.WriteHeader(http.StatusTeapot)
		})

		deleted, err := adapter.DeleteSubscriberMessages(context.Background(), subscriber)
		if deleted != 0 || err == nil {
			t.Fatalf("DeleteSubscriberMessages() with answer %v = (%d, %v), want (0, error)", answer, deleted, err)
		}
		mu.Lock()
		got := append([]string(nil), requests...)
		mu.Unlock()
		if len(got) != 1 || got[0] != "GET /v2/subscribers/"+subscriber {
			t.Fatalf("requests for answer %v = %v, want only the subscriber lookup", answer, got)
		}
	}
}
