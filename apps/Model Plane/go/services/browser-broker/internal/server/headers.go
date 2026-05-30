package server

import (
	"context"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
)

// internalHeaderPrefixes are gRPC metadata key prefixes considered
// internal to the Triode control/model plane. They must never be
// propagated to external callers.
var internalHeaderPrefixes = []string{
	"x-internal-",
	"x-triode-internal-",
}

// isInternalKey reports whether a metadata key matches any internal prefix.
// gRPC metadata keys are normalized to lowercase, but we lowercase defensively.
func isInternalKey(k string) bool {
	lk := strings.ToLower(k)
	for _, p := range internalHeaderPrefixes {
		if strings.HasPrefix(lk, p) {
			return true
		}
	}
	return false
}

// ScrubInternal returns a copy of md with all internal-prefixed keys removed.
// The input md is not mutated. A nil input returns a nil map.
func ScrubInternal(md metadata.MD) metadata.MD {
	if md == nil {
		return nil
	}
	out := metadata.MD{}
	for k, v := range md {
		if isInternalKey(k) {
			continue
		}
		cp := make([]string, len(v))
		copy(cp, v)
		out[k] = cp
	}
	return out
}

// SendSafeHeader scrubs internal-prefixed metadata keys from md and
// forwards the remaining headers via grpc.SetHeader. Handlers should use
// this in place of grpc.SetHeader when surfacing metadata to clients.
func SendSafeHeader(ctx context.Context, md metadata.MD) error {
	return grpc.SetHeader(ctx, ScrubInternal(md))
}

// UnaryHeaderScrubInterceptor returns a server interceptor that defensively
// strips internal-prefixed response headers set by downstream handlers
// before the response is sent to the external client.
func UnaryHeaderScrubInterceptor() grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req interface{}, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (interface{}, error) {
		resp, err := handler(ctx, req)
		if md, ok := metadata.FromOutgoingContext(ctx); ok && md != nil {
			scrubbed := ScrubInternal(md)
			_ = grpc.SetHeader(ctx, scrubbed)
		}
		return resp, err
	}
}
