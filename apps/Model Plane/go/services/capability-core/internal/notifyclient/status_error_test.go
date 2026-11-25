package notifyclient

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// A refusal carries its status and nothing else: the body notification-core sent
// is never in the error, so no notice text can reach a log line.
func TestAccept_NonSuccessReturnsAStatusErrorWithoutTheBody(t *testing.T) {
	const body = "notice-body-that-must-not-leak"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, body, http.StatusBadGateway)
	}))
	defer server.Close()

	client := &Client{
		BaseURL:      server.URL,
		ServiceToken: "unit-test-shared-secret-value-32bytes-minimum",
		HTTP:         server.Client(),
	}
	err := client.Accept(context.Background(), Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           "modelplane.run_completed",
	})
	var refusal *StatusError
	if !errors.As(err, &refusal) || refusal.Status != http.StatusBadGateway {
		t.Fatalf("err = %v, want a *StatusError carrying 502", err)
	}
	if strings.Contains(err.Error(), body) {
		t.Fatalf("error text carries the response body: %q", err.Error())
	}
	if want := "notifyclient: notification-core returned 502"; err.Error() != want {
		t.Fatalf("error = %q, want %q", err.Error(), want)
	}
}

func TestStatusOfReadsAPointerValueOrWrappedStatusError(t *testing.T) {
	cases := []struct {
		name string
		err  error
		want int
		ok   bool
	}{
		{"a pointer", &StatusError{Status: http.StatusForbidden}, http.StatusForbidden, true},
		{"a value", StatusError{Status: http.StatusNotFound}, http.StatusNotFound, true},
		{"a wrapped pointer", fmt.Errorf("delegate: %w", &StatusError{Status: http.StatusBadGateway}), http.StatusBadGateway, true},
		{"a transport error", errors.New("dial tcp: connection refused"), 0, false},
		{"nil", nil, 0, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, ok := StatusOf(tc.err)
			if got != tc.want || ok != tc.ok {
				t.Fatalf("StatusOf(%v) = (%d, %v), want (%d, %v)", tc.err, got, ok, tc.want, tc.ok)
			}
		})
	}
}

func TestIsPermanentOnlyForRefusalsThatNeverWin(t *testing.T) {
	cases := []struct {
		name string
		err  error
		want bool
	}{
		{"400 is permanent", &StatusError{Status: http.StatusBadRequest}, true},
		{"403 is permanent", &StatusError{Status: http.StatusForbidden}, true},
		{"404 is permanent", &StatusError{Status: http.StatusNotFound}, true},
		{"408 is worth another try", &StatusError{Status: http.StatusRequestTimeout}, false},
		{"429 is worth another try", &StatusError{Status: http.StatusTooManyRequests}, false},
		{"500 is worth another try", &StatusError{Status: http.StatusInternalServerError}, false},
		{"502 is worth another try", &StatusError{Status: http.StatusBadGateway}, false},
		{"a value 403 is permanent", StatusError{Status: http.StatusForbidden}, true},
		{"a wrapped 403 is permanent", fmt.Errorf("delegate: %w", &StatusError{Status: http.StatusForbidden}), true},
		{"a transport error is not", errors.New("dial tcp: connection refused"), false},
		{"nil is not", nil, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := IsPermanent(tc.err); got != tc.want {
				t.Fatalf("IsPermanent(%v) = %v, want %v", tc.err, got, tc.want)
			}
		})
	}
}
