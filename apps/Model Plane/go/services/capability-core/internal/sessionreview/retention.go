package sessionreview

import "encoding/json"

// RetentionPosture is a run's Zero Data Retention posture as *declared by the
// run-lifecycle envelope itself*.
//
// # Why this is parsed from raw bytes instead of read off envelope.Envelope
//
// The learning review reads a customer conversation and distils it into a
// durable `agent_skills` row, so it is a content-persisting boundary and the
// platform rule applies: Zero Data Retention must propagate across it. That
// requires knowing the run's posture — and today nothing hands it to us:
//
//   - `pkg/envelope.Envelope` has NO `zdr` field, even though the canonical
//     proto `Event` carries one (`model_plane.v1.Event` field 13) and the Rust
//     twin does too (`mp-events/src/envelope.rs`). The Go struct is simply not
//     at parity, so unmarshalling into it silently discards any `zdr` the wire
//     actually carried.
//   - `natsx.Publisher.Publish` performs no ZDR suppression, unlike
//     model-gateway's Rust `DynPublisher`, which drops `zdr: true` envelopes
//     before any backend. There is NO upstream guard on the Go publish path.
//   - session-core's `runs` table has no `zdr` column, so there is no durable
//     record to fall back on either.
//
// Parsing the raw envelope bytes for `zdr` therefore reads whatever the
// producer actually sent, independently of the lossy Go struct, and lets an
// ABSENT flag stay distinguishable from an explicit `false`. That distinction is
// the whole point: "absent" means nobody has asserted the run is retainable, and
// a privacy control that cannot verify its precondition must refuse.
type RetentionPosture int

const (
	// RetentionUnspecified — the envelope declares no `zdr` at all. Treated as
	// unsafe: see [RetentionPosture.AllowsDerivedPersistence].
	RetentionUnspecified RetentionPosture = iota
	// RetentionZeroData — the envelope declares `zdr: true`. No content derived
	// from this run may become durable.
	RetentionZeroData
	// RetentionDurable — the envelope declares `zdr: false`. Retention is
	// explicitly permitted, so a review may run.
	RetentionDurable
)

func (p RetentionPosture) String() string {
	switch p {
	case RetentionZeroData:
		return "zero_data_retention"
	case RetentionDurable:
		return "durable"
	default:
		return "unspecified"
	}
}

// AllowsDerivedPersistence reports whether a skill may be derived from — and
// therefore whether a transcript may even be READ for — a run with this
// posture.
//
// Only an explicit `zdr: false` qualifies. `RetentionUnspecified` fails closed
// on purpose, and is not an oversight to be "fixed" by defaulting to true:
//
//   - Defaulting an unstated posture to "retainable" is how a ZDR run's content
//     silently becomes a durable skill, which is precisely the leak the platform
//     rule exists to prevent. The producer side has no suppression and no `zdr`
//     field (see [RetentionPosture]), so "absent" carries no assurance whatsoever.
//   - capability-core already enforces exactly this default elsewhere: gRPC
//     capability promotion denies a caller whose retention posture is
//     unspecified, not merely one that is explicitly ZDR
//     (`internal/server`, promotion requires a verified non-ZDR posture).
//     Fail-closed-on-absence is the established house rule, not a new one.
//
// Consequence, stated plainly: until the RUN_COMPLETED producer stamps `zdr` on
// the envelope, every review is skipped and no skills are learned. That is the
// honest outcome — an inert loop is recoverable, a leaked skill is not.
func (p RetentionPosture) AllowsDerivedPersistence() bool {
	return p == RetentionDurable
}

// parseRetentionPosture probes raw envelope bytes for the `zdr` flag.
//
// Uses *bool so an absent field stays distinct from `false`. Invalid JSON maps
// to [RetentionUnspecified] — the fail-closed value — though callers decode the
// envelope first and reject undecodable payloads before reaching here.
func parseRetentionPosture(data []byte) RetentionPosture {
	var probe struct {
		ZDR *bool `json:"zdr"`
	}
	if err := json.Unmarshal(data, &probe); err != nil || probe.ZDR == nil {
		return RetentionUnspecified
	}
	if *probe.ZDR {
		return RetentionZeroData
	}
	return RetentionDurable
}
