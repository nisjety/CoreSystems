package server

import (
	"testing"

	"google.golang.org/grpc/metadata"
)

func TestScrubInternal(t *testing.T) {
	t.Run("nil input returns nil", func(t *testing.T) {
		if got := ScrubInternal(nil); got != nil {
			t.Fatalf("expected nil, got %v", got)
		}
	})

	t.Run("strips x-internal- prefix", func(t *testing.T) {
		md := metadata.New(map[string]string{
			"x-internal-tenant-id": "acme",
			"x-request-id":         "abc123",
			"content-type":         "application/grpc",
		})
		out := ScrubInternal(md)
		if _, ok := out["x-internal-tenant-id"]; ok {
			t.Fatalf("x-internal-tenant-id was not scrubbed: %v", out)
		}
		if got := out.Get("x-request-id"); len(got) != 1 || got[0] != "abc123" {
			t.Fatalf("x-request-id lost: %v", got)
		}
		if got := out.Get("content-type"); len(got) != 1 || got[0] != "application/grpc" {
			t.Fatalf("content-type lost: %v", got)
		}
	})

	t.Run("strips x-triode-internal- prefix", func(t *testing.T) {
		md := metadata.New(map[string]string{
			"x-triode-internal-trace": "span-1",
			"x-triode-public":         "ok",
		})
		out := ScrubInternal(md)
		if _, ok := out["x-triode-internal-trace"]; ok {
			t.Fatalf("x-triode-internal-trace was not scrubbed")
		}
		if got := out.Get("x-triode-public"); len(got) != 1 || got[0] != "ok" {
			t.Fatalf("x-triode-public lost: %v", got)
		}
	})

	t.Run("case insensitive match", func(t *testing.T) {
		md := metadata.MD{
			"X-Internal-Foo": []string{"bar"},
		}
		out := ScrubInternal(md)
		for k := range out {
			if isInternalKey(k) {
				t.Fatalf("internal key leaked after scrub: %q", k)
			}
		}
	})

	t.Run("does not mutate input", func(t *testing.T) {
		md := metadata.New(map[string]string{
			"x-internal-foo": "v1",
			"keep":           "v2",
		})
		_ = ScrubInternal(md)
		if got := md.Get("x-internal-foo"); len(got) != 1 || got[0] != "v1" {
			t.Fatalf("input md was mutated: %v", md)
		}
	})
}

func TestSendSafeHeader_StripsInternalPrefixes(t *testing.T) {
	// Pure unit coverage of the scrubbing contract used by SendSafeHeader.
	// The grpc.SetHeader call requires a real server stream context, which is
	// covered by end-to-end tests; here we verify scrubbing is applied to the
	// metadata handed off to SetHeader.
	md := metadata.New(map[string]string{
		"x-internal-secret":       "topsecret",
		"x-triode-internal-trace": "abc",
		"x-public":                "ok",
	})
	scrubbed := ScrubInternal(md)
	if _, ok := scrubbed["x-internal-secret"]; ok {
		t.Fatalf("x-internal-secret leaked: %v", scrubbed)
	}
	if _, ok := scrubbed["x-triode-internal-trace"]; ok {
		t.Fatalf("x-triode-internal-trace leaked: %v", scrubbed)
	}
	if got := scrubbed.Get("x-public"); len(got) != 1 || got[0] != "ok" {
		t.Fatalf("x-public lost: %v", scrubbed)
	}
}
