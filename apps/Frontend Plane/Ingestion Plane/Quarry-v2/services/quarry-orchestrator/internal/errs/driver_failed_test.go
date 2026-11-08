package errs

import "testing"

// The closed list and its flags, as quarry-core pins them in error.rs
// (driver_failed_reason::retryable). A change on either side fails here.
func TestDriverFailedVocabularyMatchesTheEdge(t *testing.T) {
	cases := []struct {
		reason    string
		retryable bool
	}{
		{"dns", true},
		{"tls", false},
		{"connect", true},
		{"all_drivers_failed", false},
	}
	for _, c := range cases {
		retryable, ok := DriverFailedRetryable(c.reason)
		if !ok {
			t.Errorf("%s is not a fetch sub-code", c.reason)
			continue
		}
		if retryable != c.retryable {
			t.Errorf("%s retryable = %v, want %v", c.reason, retryable, c.retryable)
		}
	}
}

func TestDriverFailedRetryable_RefusesAnythingOutsideTheList(t *testing.T) {
	for _, reason := range []string{"", "DNS", "action_failed", "connection reset by https://example.test"} {
		if _, ok := DriverFailedRetryable(reason); ok {
			t.Errorf("%q must not be a fetch sub-code", reason)
		}
	}
}

func TestDriverFailedReasonOf_ReadsASubCodeOfADriverFailed(t *testing.T) {
	body := []byte(`{"error":{"code":"DRIVER_FAILED","message":"x","details":{"reason":"dns"},"retryable":true}}`)
	reason, ok := DriverFailedReasonOf(body)
	if !ok || reason != "dns" {
		t.Fatalf("DriverFailedReasonOf = (%q, %v), want (\"dns\", true)", reason, ok)
	}
}

func TestDriverFailedReasonOf_IgnoresOtherCodesReasonsAndBodies(t *testing.T) {
	cases := []struct {
		name string
		body string
	}{
		{"another code", `{"error":{"code":"TIMEOUT","details":{"reason":"dns"}}}`},
		{"typed reason", `{"error":{"code":"DRIVER_FAILED","details":{"reason":"action_failed"}}}`},
		{"no reason", `{"error":{"code":"DRIVER_FAILED"}}`},
		{"non-string reason", `{"error":{"code":"DRIVER_FAILED","details":{"reason":7}}}`},
		{"not json", `<html>`},
	}
	for _, c := range cases {
		if reason, ok := DriverFailedReasonOf([]byte(c.body)); ok {
			t.Errorf("%s: got sub-code %q", c.name, reason)
		}
	}
}
