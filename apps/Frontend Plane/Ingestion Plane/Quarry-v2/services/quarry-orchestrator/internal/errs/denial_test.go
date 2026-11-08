package errs

import (
	"errors"
	"testing"

	"go.temporal.io/sdk/temporal"
)

// A 403 from the edge whose envelope carries details.denial is a blocklist refusal (WS14.S3).
const denialBody = `{"error":{"code":"SECURITY_BLOCKED","message":"refused","details":{"denial":{"code":"robots_disallowed","message":"blocked by robots.txt","remedy":"allow the path","path":"/private"}}}}`

func TestFromHTTPResponse_DenialBecomesBlocklistAndKeepsTheDenial(t *testing.T) {
	e := FromHTTPResponse("activities.RunPage", 403, []byte(denialBody))
	if e.Category != CategoryBlocklist {
		t.Fatalf("category = %v, want blocklist", e.Category)
	}
	if e.Category.Retryable() {
		t.Error("a denial must not be retried")
	}
	if e.Denial["code"] != "robots_disallowed" {
		t.Errorf("denial code = %v", e.Denial["code"])
	}
}

func TestFromHTTPResponse_PlainForbiddenStaysAuth(t *testing.T) {
	e := FromHTTPResponse("activities.RunPage", 403, []byte(`{"error":{"code":"FORBIDDEN"}}`))
	if e.Category != CategoryAuth {
		t.Fatalf("category = %v, want auth", e.Category)
	}
	if e.Denial != nil {
		t.Errorf("no denial expected, got %v", e.Denial)
	}
}

func TestFromHTTPResponse_SecurityBlockedWithoutCodeIsNotADenial(t *testing.T) {
	e := FromHTTPResponse("activities.RunPage", 403, []byte(`{"error":{"code":"SECURITY_BLOCKED","details":{"denial":{}}}}`))
	if e.Category == CategoryBlocklist {
		t.Fatal("a denial without a code must not be classified as a denial")
	}
}

func TestCategoryOf_ReadsTheTemporalTypeAndTheDenialSurvivesTheConversion(t *testing.T) {
	e := FromHTTPResponse("activities.RunPage", 403, []byte(denialBody))
	err := e.Temporal()

	if got := CategoryOf(err); got != CategoryBlocklist {
		t.Fatalf("CategoryOf over temporal error = %v, want blocklist", got)
	}
	var appErr *temporal.ApplicationError
	if !errors.As(err, &appErr) {
		t.Fatal("Temporal() must return an application error")
	}
	denial, ok := DenialOf(err)
	if !ok {
		t.Fatal("DenialOf must read the denial back from the application error details")
	}
	if denial["code"] != "robots_disallowed" || denial["remedy"] != "allow the path" {
		t.Errorf("denial after conversion = %v", denial)
	}
}

func TestCategoryOf_UnclassifiedErrorIsUnknown(t *testing.T) {
	if got := CategoryOf(errors.New("plain")); got != CategoryUnknown {
		t.Fatalf("CategoryOf plain error = %v, want unknown", got)
	}
	if _, ok := DenialOf(errors.New("plain")); ok {
		t.Error("a plain error carries no denial")
	}
}
