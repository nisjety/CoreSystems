package errs

import (
	"errors"
	"testing"

	"go.temporal.io/sdk/temporal"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func TestFromHTTPStatus(t *testing.T) {
	cases := []struct {
		name    string
		status  int
		want    Category
		retryOK bool
	}{
		{"network", 0, CategoryNetwork, true},
		{"unauthorized", 401, CategoryAuth, false},
		{"forbidden", 403, CategoryAuth, false},
		{"rate_limited", 429, CategoryServer5xx, true},
		{"client_404", 404, CategoryClient4xx, false},
		{"server_500", 500, CategoryServer5xx, true},
		{"server_503", 503, CategoryServer5xx, true},
		{"odd_high", 700, CategoryNetwork, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := FromHTTPStatus("RunPage", tc.status, []byte("body"))
			e, ok := Classify(err)
			if !ok {
				t.Fatalf("expected classified error, got %T", err)
			}
			if e.Category != tc.want {
				t.Fatalf("category=%s want=%s", e.Category, tc.want)
			}
			if e.Category.Retryable() != tc.retryOK {
				t.Fatalf("retryable=%v want=%v", e.Category.Retryable(), tc.retryOK)
			}
		})
	}
}

func TestTemporalWrapNonRetryable(t *testing.T) {
	e := New(CategoryClient4xx, "RunPage", errors.New("bad request"))
	err := e.Temporal()
	var appErr *temporal.ApplicationError
	if !errors.As(err, &appErr) {
		t.Fatalf("expected ApplicationError, got %T", err)
	}
	if !appErr.NonRetryable() {
		t.Fatalf("expected non-retryable")
	}
	if appErr.Type() != "client_4xx" {
		t.Fatalf("type=%s", appErr.Type())
	}
}

func TestTemporalWrapRetryable(t *testing.T) {
	e := New(CategoryServer5xx, "RunPage", errors.New("boom"))
	err := e.Temporal()
	var appErr *temporal.ApplicationError
	if !errors.As(err, &appErr) {
		t.Fatalf("expected ApplicationError, got %T", err)
	}
	if appErr.NonRetryable() {
		t.Fatalf("expected retryable")
	}
}

func TestCategoryEvent(t *testing.T) {
	cases := map[Category]quarrycontracts.EventType{
		CategoryBlocklist:  quarrycontracts.EvtPageBlocked,
		CategoryClient4xx:  quarrycontracts.EvtPageFailed,
		CategoryValidation: quarrycontracts.EvtPageFailed,
		CategoryAuth:       quarrycontracts.EvtPageFailed,
		CategoryNetwork:    quarrycontracts.EvtPageRetried,
		CategoryTimeout:    quarrycontracts.EvtPageRetried,
		CategoryServer5xx:  quarrycontracts.EvtPageRetried,
	}
	for c, want := range cases {
		if got := c.Event(); got != want {
			t.Fatalf("category=%s event=%s want=%s", c, got, want)
		}
	}
}

func TestIsNonRetryable(t *testing.T) {
	nr := New(CategoryBlocklist, "RunPage", errors.New("blocked")).Temporal()
	if !IsNonRetryable(nr) {
		t.Fatalf("expected non-retryable")
	}
	r := New(CategoryNetwork, "RunPage", errors.New("timeout")).Temporal()
	if IsNonRetryable(r) {
		t.Fatalf("expected retryable")
	}
	if IsNonRetryable(nil) {
		t.Fatalf("nil should not be non-retryable")
	}
}
