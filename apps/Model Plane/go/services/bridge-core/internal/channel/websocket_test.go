package channel

import (
	"context"
	"errors"
	"testing"
)

func TestWebSocketSkeletonFailsClosedWithoutAuthenticatedUpgradeProtocol(t *testing.T) {
	adapter := NewWebSocketAdapter(JSONFrameCodec{})
	if _, err := adapter.Ingest(context.Background(), "session-a", []byte(`{"payload":"unsafe"}`)); !errors.Is(err, ErrWebSocketUnavailable) {
		t.Fatalf("error = %v, want ErrWebSocketUnavailable", err)
	}
}
