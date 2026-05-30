// Package natsx — subscriber.go provides a mode-aware subscribe wrapper that
// fans out across v1 and legacy subjects based on CompatMode, deduplicating
// by envelope EventID in ModeDualRead.
package natsx

import (
	"fmt"
	"sync"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// MsgHandler is the user-provided delivery callback. The subject passed in
// is always normalized to the canonical mp.v1.* form regardless of which
// underlying subject the message arrived on.
type MsgHandler func(subject string, data []byte) error

// RawMsgHandler is the callback type a RawSubscriber delivers.
type RawMsgHandler func(subject string, data []byte)

// Subscription is the minimal handle returned by RawSubscriber.Subscribe.
// A thin adapter over *nats.Subscription satisfies this.
type Subscription interface {
	Unsubscribe() error
}

// RawSubscriber is the minimal subscribe surface required by the wrapper.
type RawSubscriber interface {
	Subscribe(subject string, handler RawMsgHandler) (Subscription, error)
}

// Subscriber wraps a RawSubscriber with compat-mode dispatch. Callers always
// subscribe using the canonical mp.v1.* subject; the wrapper fans out to
// legacy subjects and dedups by envelope EventID when in ModeDualRead.
type Subscriber struct {
	raw  RawSubscriber
	mode CompatMode
	seen sync.Map // eventID -> struct{}
}

// NewSubscriber returns a Subscriber bound to the given raw connection and mode.
func NewSubscriber(raw RawSubscriber, mode CompatMode) *Subscriber {
	return &Subscriber{raw: raw, mode: mode}
}

// Mode returns the active compat mode.
func (s *Subscriber) Mode() CompatMode { return s.mode }

// Subscribe registers handler for the given v1 subject. Dispatch:
//
//	ModeV1Only, ModeDualWrite — subscribe v1 only
//	ModeDualRead              — subscribe v1 plus legacy mirror (when a mapping
//	                            exists), deduping by envelope EventID and
//	                            normalizing arrival subject to v1 form
//	ModeLegacyOnly            — subscribe legacy only; error if no mapping
//
// Returns every active subscription so callers can unsubscribe on shutdown.
func (s *Subscriber) Subscribe(v1Subject string, handler MsgHandler) ([]Subscription, error) {
	switch s.mode {
	case ModeV1Only, ModeDualWrite:
		sub, err := s.raw.Subscribe(v1Subject, s.invokeHandler(handler))
		if err != nil {
			return nil, fmt.Errorf("natsx: subscribe v1 %q: %w", v1Subject, err)
		}
		return []Subscription{sub}, nil

	case ModeDualRead:
		subs := make([]Subscription, 0, 2)

		v1Sub, err := s.raw.Subscribe(v1Subject, s.dedupHandler(handler))
		if err != nil {
			return nil, fmt.Errorf("natsx: subscribe v1 %q: %w", v1Subject, err)
		}
		subs = append(subs, v1Sub)

		legacySubject := TranslateNewToLegacy(v1Subject)
		if legacySubject == "" {
			return subs, nil
		}
		legacySub, err := s.raw.Subscribe(legacySubject, s.dedupHandler(handler))
		if err != nil {
			_ = v1Sub.Unsubscribe()
			return nil, fmt.Errorf("natsx: subscribe legacy %q: %w", legacySubject, err)
		}
		subs = append(subs, legacySub)
		return subs, nil

	case ModeLegacyOnly:
		legacySubject := TranslateNewToLegacy(v1Subject)
		if legacySubject == "" {
			return nil, fmt.Errorf("natsx: no legacy mapping for %q in legacy_only mode", v1Subject)
		}
		sub, err := s.raw.Subscribe(legacySubject, s.invokeHandler(handler))
		if err != nil {
			return nil, fmt.Errorf("natsx: subscribe legacy %q: %w", legacySubject, err)
		}
		return []Subscription{sub}, nil

	default:
		return nil, fmt.Errorf("natsx: unknown compat mode %d", s.mode)
	}
}

// invokeHandler wraps handler for modes without dedup. It still normalizes
// the arrival subject to canonical v1 form so handler code is mode-agnostic.
func (s *Subscriber) invokeHandler(handler MsgHandler) RawMsgHandler {
	return func(arrivalSubject string, data []byte) {
		_ = handler(TranslateLegacySubject(arrivalSubject), data)
	}
}

// dedupHandler returns a raw handler that dedups by envelope EventID and
// invokes the user handler with the normalized v1 subject. On decode
// failures it fails open: handler is still invoked so operators can observe
// malformed messages rather than silently dropping them during migration.
func (s *Subscriber) dedupHandler(handler MsgHandler) RawMsgHandler {
	return func(arrivalSubject string, data []byte) {
		normalized := TranslateLegacySubject(arrivalSubject)
		env, err := envelope.Decode(data)
		if err != nil {
			_ = handler(normalized, data)
			return
		}
		if env.EventID != "" {
			if _, dup := s.seen.LoadOrStore(env.EventID, struct{}{}); dup {
				return
			}
		}
		_ = handler(normalized, data)
	}
}
