package sessionreview

import (
	"fmt"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"testing"
	"time"
)

func TestReviewRetryPolicy(t *testing.T) {
	if reviewRetryDelay(1) != 30*time.Second || reviewRetryDelay(4) != 30*time.Minute || reviewRetryDelay(100) != 30*time.Minute {
		t.Fatal("redelivery must be delayed and bounded")
	}
	for _, code := range []codes.Code{codes.PermissionDenied, codes.Unauthenticated, codes.InvalidArgument, codes.FailedPrecondition} {
		if !permanentReviewFailure(fmt.Errorf("wrapped: %w", status.Error(code, "detail"))) {
			t.Fatalf("configuration/authorization failure must not spin: %s", code)
		}
	}
	if permanentReviewFailure(status.Error(codes.Unavailable, "outage")) || permanentReviewFailure(fmt.Errorf("invalid schema")) {
		t.Fatal("transient failures should retry")
	}
}
