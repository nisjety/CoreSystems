package integration

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// newTestClient points a Client at an httptest server with a fast timeout so
// the timeout test does not stall the suite.
func newTestClient(t *testing.T, baseURL string) *Client {
	t.Helper()
	return NewClient(baseURL, "test-key", WithHTTPClient(&http.Client{Timeout: 200 * time.Millisecond}))
}

func TestSend_Success_ReturnsProviderMessageID(t *testing.T) {
	var gotPath, gotKey, gotOrg, gotUser, gotOperation string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotKey = r.Header.Get("x-internal-api-key")
		gotOrg = r.Header.Get("x-org-id")
		gotUser = r.Header.Get("x-user-id")
		body, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(body, &parsed)
		gotOperation = parsed.Operation
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"slack","operation":"message.send","result":{"ts":"1700000000.000100"}}}}`))
	}))
	defer srv.Close()

	cl := newTestClient(t, srv.URL)
	res, err := cl.Send(context.Background(), SendRequest{
		OrgID:            "org-1",
		ActorUserID:      "user-7",
		Provider:         "slack",
		ConnectionID:     "conn-9",
		ProviderThreadID: "C123",
		BodyText:         "hello",
	})
	if err != nil {
		t.Fatalf("Send err = %v, want nil", err)
	}
	if res.ProviderMessageID != "1700000000.000100" {
		t.Errorf("provider_message_id = %q, want the slack ts", res.ProviderMessageID)
	}
	if gotPath != "/api/v1/connections/conn-9/actions" {
		t.Errorf("path = %q, want connections actions path", gotPath)
	}
	if gotKey != "test-key" {
		t.Errorf("internal api key header = %q, want test-key", gotKey)
	}
	if gotOrg != "org-1" || gotUser != "user-7" {
		t.Errorf("org/user headers = %q/%q, want org-1/user-7", gotOrg, gotUser)
	}
	if gotOperation != "message.send" {
		t.Errorf("operation = %q, want message.send for slack", gotOperation)
	}
}

func TestSend_NestedSlackMessageTS(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"message":{"ts":"42.99"}}}}}`))
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	res, err := cl.Send(context.Background(), SendRequest{Provider: "slack", ConnectionID: "c1", BodyText: "x"})
	if err != nil {
		t.Fatalf("Send err = %v", err)
	}
	if res.ProviderMessageID != "42.99" {
		t.Errorf("nested message.ts = %q, want 42.99", res.ProviderMessageID)
	}
}

func TestSend_Success_EmptyBodyIsStillSuccess(t *testing.T) {
	// Graph sendMail returns 202 with no body — must still be a success, just
	// without a provider message id.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusAccepted)
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	res, err := cl.Send(context.Background(), SendRequest{Provider: "microsoft", ConnectionID: "c1", BodyText: "x", To: []string{"a@b.no"}})
	if err != nil {
		t.Fatalf("Send err = %v, want nil for 202", err)
	}
	if res.ProviderMessageID != "" {
		t.Errorf("provider_message_id = %q, want empty for no-body 202", res.ProviderMessageID)
	}
	if res.Operation != "mail.send" {
		t.Errorf("operation = %q, want mail.send for microsoft", res.Operation)
	}
}

func TestSend_4xx_IsTerminal(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"success":false,"error":{"code":"invalid_body","message":"bad"}}`))
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", ConnectionID: "c1", BodyText: "x"})
	if err == nil {
		t.Fatal("Send err = nil, want terminal error")
	}
	if !IsTerminal(err) {
		t.Errorf("4xx not classified terminal: %v", err)
	}
}

func TestSend_5xx_IsTransient(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
		_, _ = w.Write([]byte(`{"success":false,"error":{"code":"upstream","message":"down"}}`))
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", ConnectionID: "c1", BodyText: "x"})
	if err == nil {
		t.Fatal("Send err = nil, want transient error")
	}
	if IsTerminal(err) {
		t.Errorf("5xx classified terminal, want transient: %v", err)
	}
}

func TestSend_Timeout_IsTransient(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		time.Sleep(500 * time.Millisecond) // exceeds the 200ms client timeout
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", ConnectionID: "c1", BodyText: "x"})
	if err == nil {
		t.Fatal("Send err = nil, want transient timeout error")
	}
	if IsTerminal(err) {
		t.Errorf("timeout classified terminal, want transient: %v", err)
	}
}

func TestSend_UnsupportedProvider_IsTerminal(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		t.Error("server must not be hit for an unsupported provider")
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), SendRequest{Provider: "fax", ConnectionID: "c1", BodyText: "x"})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("unsupported provider err = %v, want terminal", err)
	}
}

func TestSend_NotConfigured_IsTerminal(t *testing.T) {
	cl := NewClient("", "")
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", ConnectionID: "c1", BodyText: "x"})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("unconfigured client err = %v, want terminal", err)
	}
}

func TestSend_MissingConnection_IsTerminal(t *testing.T) {
	cl := NewClient("https://integration.local", "k")
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", BodyText: "x"})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("missing connection err = %v, want terminal", err)
	}
}

func TestSend_WhatsApp_UsesCloudAPIShape(t *testing.T) {
	var gotOp string
	var gotParams, gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(raw, &parsed)
		gotOp, gotParams, gotBody = parsed.Operation, parsed.Params, parsed.Body
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"messages":[{"id":"wamid.HBgL123"}]}}}}`))
	}))
	defer srv.Close()

	res, err := newTestClient(t, srv.URL).Send(context.Background(), SendRequest{
		Provider:         "whatsapp",
		ConnectionID:     "c1",
		ProviderThreadID: "1067xxxxphone:4790012345",
		BodyText:         "Hei!",
	})
	if err != nil {
		t.Fatalf("whatsapp send err = %v", err)
	}
	if gotOp != "whatsapp.messages.send" {
		t.Errorf("operation = %q, want whatsapp.messages.send", gotOp)
	}
	if gotParams["phoneNumberId"] != "1067xxxxphone" {
		t.Errorf("phoneNumberId = %v, want the business id from the composite thread id", gotParams["phoneNumberId"])
	}
	if gotBody["to"] != "4790012345" || gotBody["type"] != "text" {
		t.Errorf("body = %#v, want to=recipient type=text", gotBody)
	}
	if txt, _ := gotBody["text"].(map[string]any); txt["body"] != "Hei!" {
		t.Errorf("text.body = %#v, want the message text", gotBody["text"])
	}
	if res.ProviderMessageID != "wamid.HBgL123" {
		t.Errorf("provider_message_id = %q, want the wamid from messages[0].id", res.ProviderMessageID)
	}
}

func TestSend_Messenger_UsesSendAPIShape(t *testing.T) {
	var gotOp string
	var gotParams, gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(raw, &parsed)
		gotOp, gotParams, gotBody = parsed.Operation, parsed.Params, parsed.Body
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"message_id":"m_AG5Hc2"}}}}`))
	}))
	defer srv.Close()

	res, err := newTestClient(t, srv.URL).Send(context.Background(), SendRequest{
		Provider:         "messenger",
		ConnectionID:     "c1",
		ProviderThreadID: "1094page:73940psid",
		BodyText:         "Takk for meldingen",
	})
	if err != nil {
		t.Fatalf("messenger send err = %v", err)
	}
	if gotOp != "messenger.messages.send" {
		t.Errorf("operation = %q, want messenger.messages.send", gotOp)
	}
	if gotParams["pageId"] != "1094page" {
		t.Errorf("pageId = %v, want the page id from the composite thread id", gotParams["pageId"])
	}
	recip, _ := gotBody["recipient"].(map[string]any)
	if recip["id"] != "73940psid" {
		t.Errorf("recipient.id = %#v, want the PSID", gotBody["recipient"])
	}
	if res.ProviderMessageID != "m_AG5Hc2" {
		t.Errorf("provider_message_id = %q, want the messenger message_id", res.ProviderMessageID)
	}
}

func TestSend_WhatsApp_MissingBusinessID_IsTerminal(t *testing.T) {
	// No ":" in the thread id → no business phone-number id → cannot route.
	_, err := newTestClient(t, "http://unused").Send(context.Background(), SendRequest{
		Provider:         "whatsapp",
		ConnectionID:     "c1",
		ProviderThreadID: "4790012345",
		BodyText:         "hi",
	})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("err = %v, want terminal for missing business id", err)
	}
}

func TestSend_Discord_IsHonestlyUnsupported(t *testing.T) {
	_, err := newTestClient(t, "http://unused").Send(context.Background(), SendRequest{
		Provider: "discord", ConnectionID: "c1", BodyText: "hi",
	})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("err = %v, want terminal for discord", err)
	}
}

func TestSend_PerProviderOperationMapping(t *testing.T) {
	cases := map[string]string{
		"microsoft": "mail.send",
		"slack":     "message.send",
		"google":    "gmail.send",
	}
	for provider, wantOp := range cases {
		var gotOp string
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			body, _ := io.ReadAll(r.Body)
			var parsed actionRequestBody
			_ = json.Unmarshal(body, &parsed)
			gotOp = parsed.Operation
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"id":"m1"}}}}`))
		}))
		_, err := newTestClient(t, srv.URL).Send(context.Background(), SendRequest{
			Provider: provider, ConnectionID: "c1", BodyText: "hi", To: []string{"x@y.no"},
		})
		srv.Close()
		if err != nil {
			t.Fatalf("provider %s send err = %v", provider, err)
		}
		if !strings.EqualFold(gotOp, wantOp) {
			t.Errorf("provider %s operation = %q, want %q", provider, gotOp, wantOp)
		}
	}
}
