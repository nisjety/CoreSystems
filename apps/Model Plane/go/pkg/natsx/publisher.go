// Package natsx — publisher.go provides a mode-aware publish wrapper that
// dispatches v1 subjects to v1 and/or legacy subjects based on CompatMode.
package natsx

import (
	"fmt"
	"log/slog"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// RawPublisher is the minimal publish surface required by the wrapper.
// *nats.Conn satisfies this interface.
type RawPublisher interface {
	Publish(subject string, data []byte) error
}

// Publisher wraps a RawPublisher with compat-mode dispatch. Callers always
// publish using the canonical mp.v1.* subject; the wrapper translates/fans
// out to legacy subjects as dictated by the active CompatMode.
type Publisher struct {
	raw  RawPublisher
	mode CompatMode
}

// NewPublisher returns a Publisher bound to the given raw connection and mode.
func NewPublisher(raw RawPublisher, mode CompatMode) *Publisher {
	return &Publisher{raw: raw, mode: mode}
}

// Mode returns the active compat mode.
func (p *Publisher) Mode() CompatMode { return p.mode }

// Publish sends data using the v1 subject as input. Dispatch:
//
//	ModeV1Only, ModeDualRead — publish v1 only
//	ModeDualWrite            — publish v1, then legacy mirror if mapping exists
//	ModeLegacyOnly           — publish only legacy (error if no mapping)
//
// # Zero Data Retention suppression
//
// An envelope declaring `zdr: true` is dropped before ANY backend, mirroring
// model-gateway's Rust `DynPublisher::publish`. This is enforced here rather
// than at each call site because run-event subjects are JetStream-captured
// (`MODEL_PLANE_RUN_EVENTS`, 48h file-backed), so a single unguarded producer
// would put no-retention content on disk. A suppressed publish is NOT an error
// — the Rust twin returns Ok(()) — because the producer did nothing wrong and
// failing it would only turn a satisfied retention rule into a retry storm.
//
// The check reads the serialized bytes, so it holds for every caller regardless
// of how the payload was built. Payloads that are not JSON envelopes (and
// envelopes with no `zdr` key at all) declare no posture and are published
// unchanged: suppressing an undeclared event would silently delete lifecycle
// signal, and the retention decision for undeclared events belongs to the
// consumer, which fails closed on absence.
func (p *Publisher) Publish(v1Subject string, data []byte) error {
	if zdr, declared := envelope.DeclaredZDR(data); declared && zdr {
		slog.Debug("natsx: ZDR envelope suppressed before publisher backend", "subject", v1Subject)
		return nil
	}
	switch p.mode {
	case ModeV1Only, ModeDualRead:
		return p.raw.Publish(v1Subject, data)
	case ModeDualWrite:
		if err := p.raw.Publish(v1Subject, data); err != nil {
			return err
		}
		if legacy := TranslateNewToLegacy(v1Subject); legacy != "" {
			return p.raw.Publish(legacy, data)
		}
		return nil
	case ModeLegacyOnly:
		legacy := TranslateNewToLegacy(v1Subject)
		if legacy == "" {
			return fmt.Errorf("natsx: no legacy mapping for %q in legacy_only mode", v1Subject)
		}
		return p.raw.Publish(legacy, data)
	default:
		return p.raw.Publish(v1Subject, data)
	}
}
