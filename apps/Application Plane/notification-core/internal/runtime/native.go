package runtime

import (
	"context"
	"errors"
	"fmt"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// nativePayloadKeys is the closed set of payload fields a native notification may carry: fixed copy, a
// deep link and opaque ids. The native path writes this payload straight into the in-app feed, so a
// field outside the set is refused rather than stored.
var nativePayloadKeys = map[string]struct{}{
	"title":       {},
	"body":        {},
	"message_key": {},
	"cta_href":    {},
	"cta_label":   {},
	"thread_id":   {},
	"run_id":      {},
	"kind":        {},
	"outcome":     {},
}

// NativeAdapter delivers the native types inside notification-core. It makes no provider call: the feed
// mirror writes the in-app row from the same dispatch. It accepts only native types, refuses every
// zero-retention request, and refuses any payload field outside the content-free set.
type NativeAdapter struct{}

// NewNativeAdapter constructs the adapter. It needs no configuration, because there is no provider to reach.
func NewNativeAdapter() *NativeAdapter {
	return &NativeAdapter{}
}

// Dispatch accepts one native request. The provider request id names the native path, so the feed row
// and the request correlate without a provider receipt.
func (a *NativeAdapter) Dispatch(_ context.Context, req notification.DeliveryRequest) (*notification.DispatchResult, error) {
	if !notification.IsNativeType(req.Type) {
		return nil, fmt.Errorf("native delivery does not carry notification type %q", req.Type)
	}
	if req.RetentionMode == notification.RetentionModeZDR {
		return nil, errors.New("native delivery does not carry zero-data-retention requests")
	}
	for key := range req.Payload {
		if _, ok := nativePayloadKeys[key]; !ok {
			return nil, errors.New("native delivery refused a payload field outside the content-free set")
		}
	}
	return &notification.DispatchResult{
		Provider:          notification.ProviderNative,
		ProviderRequestID: "native:" + req.RequestID,
	}, nil
}

// Composite routes each request to the adapter that owns its type: the native adapter for the native
// types, and the provider adapter for everything else.
type Composite struct {
	provider notification.RuntimeClient
	native   notification.RuntimeClient
}

// NewComposite pairs the provider adapter with the native adapter.
func NewComposite(provider, native notification.RuntimeClient) *Composite {
	return &Composite{provider: provider, native: native}
}

// Dispatch sends a native type to the native adapter and every other type to the provider.
func (c *Composite) Dispatch(ctx context.Context, req notification.DeliveryRequest) (*notification.DispatchResult, error) {
	if notification.IsNativeType(req.Type) {
		return c.native.Dispatch(ctx, req)
	}
	return c.provider.Dispatch(ctx, req)
}
