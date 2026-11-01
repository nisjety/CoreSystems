package keyrevoke

import (
	"context"
	"errors"
	"strings"
	"testing"
)

type fakeRevoker struct {
	calls []string
	err   error
}

func (f *fakeRevoker) RevokeKey(_ context.Context, keyID string) (int64, error) {
	f.calls = append(f.calls, keyID)
	return 1, f.err
}

func TestProcess_RevokesTheKeyFromTopLevelOrDetails(t *testing.T) {
	for _, body := range []string{
		`{"key_id":"key_1","org_id":"org1","reason":"deleted"}`,
		`{"event_type":"api_key_revoked","details":{"key_id":"key_1","org_id":"org1"}}`,
		`{"key_id":"key_1","details":{"key_id":"key_1"}}`,
	} {
		r := &fakeRevoker{}
		if got := Process(context.Background(), r, []byte(body)); got != Ack {
			t.Fatalf("%s: outcome %v, want Ack", body, got)
		}
		if len(r.calls) != 1 || r.calls[0] != "key_1" {
			t.Fatalf("%s: calls %v", body, r.calls)
		}
	}
}

func TestProcess_MalformedIsAckedAndRevokesNothing(t *testing.T) {
	for _, body := range []string{
		``, `not json`, `{}`, `{"key_id":""}`, `{"key_id":"has space"}`, `{"key_id":"` + strings.Repeat("a", 129) + `"}`,
		`{"key_id":"k\u0000"}`, `{"key_id":"a","details":{"key_id":"b"}}`, `{"key_id":7}`,
		`{"key_id":"k","pad":"` + strings.Repeat("x", maxEventBytes) + `"}`,
	} {
		r := &fakeRevoker{}
		if got := Process(context.Background(), r, []byte(body)); got != Ack {
			t.Fatalf("%q: outcome %v, want Ack", body, got)
		}
		if len(r.calls) != 0 {
			t.Fatalf("%q: revoked %v", body, r.calls)
		}
	}
}

func TestProcess_StoreErrorRetries(t *testing.T) {
	r := &fakeRevoker{err: errors.New("store down")}
	if got := Process(context.Background(), r, []byte(`{"key_id":"key_1"}`)); got != Retry {
		t.Fatalf("outcome %v, want Retry", got)
	}
}

func TestNew_DefaultsTheStream(t *testing.T) {
	if c := New(nil, " ", &fakeRevoker{}); c.stream != DefaultStream {
		t.Fatalf("stream %q", c.stream)
	}
	if c := New(nil, "OTHER", &fakeRevoker{}); c.stream != "OTHER" {
		t.Fatalf("stream %q", c.stream)
	}
}
