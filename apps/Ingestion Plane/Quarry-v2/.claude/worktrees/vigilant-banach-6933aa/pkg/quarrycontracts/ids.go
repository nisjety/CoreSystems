// Package quarrycontracts mirrors crates/quarry-core/src in Go.
// Source of truth: docs/CONTRACTS.md. Rust side: quarry-core.
package quarrycontracts

import (
	"fmt"
	"strings"
	"time"

	"github.com/oklog/ulid/v2"
)

type IDKind string

const (
	KindRun             IDKind = "run_"
	KindQueue           IDKind = "queue_"
	KindCheckpoint      IDKind = "cp_"
	KindSchedule        IDKind = "sch_"
	KindStore           IDKind = "store_"
	KindSnapshot        IDKind = "snap_"
	KindArtifact        IDKind = "art_"
	KindLease           IDKind = "lease_"
	KindProfile         IDKind = "prof_"
	KindJob             IDKind = "job_"
	KindEvent           IDKind = "evt_"
	KindWebhook         IDKind = "whk_"
	KindWebhookDelivery IDKind = "whkd_"
	KindBlocklist       IDKind = "block_"
	KindRequest         IDKind = "req_"
)

// ID is a prefixed ULID. Stringly typed — kind tag is the prefix itself.
type ID string

func NewID(k IDKind) ID {
	u := ulid.MustNew(ulid.Timestamp(time.Now()), ulid.DefaultEntropy())
	return ID(string(k) + u.String())
}

func (id ID) Kind() (IDKind, bool) {
	for _, k := range []IDKind{
		KindRun, KindQueue, KindCheckpoint, KindSchedule, KindStore,
		KindSnapshot, KindArtifact, KindLease, KindProfile, KindJob,
		KindEvent, KindWebhook, KindWebhookDelivery, KindBlocklist, KindRequest,
	} {
		if strings.HasPrefix(string(id), string(k)) {
			return k, true
		}
	}
	return "", false
}

func (id ID) MustKind(expect IDKind) error {
	got, ok := id.Kind()
	if !ok {
		return fmt.Errorf("id %q has no known kind prefix", id)
	}
	if got != expect {
		return fmt.Errorf("id %q has kind %q, expected %q", id, got, expect)
	}
	return nil
}

func (id ID) String() string { return string(id) }
