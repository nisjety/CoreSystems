package repo

import (
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

func TestIdempotencyReuseIsBoundToTheVerifiedOwner(t *testing.T) {
	existing := &model.Document{OwnerID: "user-a", Content: "private"}
	if !idempotencyOwnerMatches(existing, "user-a") {
		t.Fatal("same owner should be allowed to reuse its idempotency key")
	}
	if idempotencyOwnerMatches(existing, "user-b") {
		t.Fatal("another same-org user must not reuse or observe the private document")
	}
}
