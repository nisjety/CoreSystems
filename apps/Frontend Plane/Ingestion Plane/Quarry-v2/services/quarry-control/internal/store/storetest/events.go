package storetest

import (
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// EventSeqConformance runs the per-run sequence contract (D2-016) against any
// EventLog: many writers of one run at once get the seqs 1..N exactly once each,
// no append is lost to a seq collision, a replayed event id is a conflict that
// consumes no seq, and a second run is numbered on its own.
func EventSeqConformance(t *testing.T, newDB func(t *testing.T) store.DB) {
	t.Helper()
	const writers = 32

	newEvent := func(run quarrycontracts.ID) quarrycontracts.Event {
		rid := run
		return quarrycontracts.Event{
			EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
			RunID:     &rid,
			Type:      quarrycontracts.EvtRunStarted,
			Timestamp: time.Now().UTC(),
			Payload:   map[string]any{"k": "v"},
		}
	}

	t.Run("concurrent appends of one run take distinct contiguous seqs", func(t *testing.T) {
		db := newDB(t)
		run := quarrycontracts.NewID(quarrycontracts.KindRun)
		var (
			wg   sync.WaitGroup
			mu   sync.Mutex
			seqs []uint64
			errs []error
		)
		start := make(chan struct{})
		for i := 0; i < writers; i++ {
			wg.Add(1)
			go func() {
				defer wg.Done()
				<-start
				seq, err := db.Events().AppendNext(newEvent(run))
				mu.Lock()
				defer mu.Unlock()
				if err != nil {
					errs = append(errs, err)
					return
				}
				seqs = append(seqs, seq)
			}()
		}
		close(start)
		wg.Wait()
		if len(errs) > 0 {
			t.Fatalf("%d of %d concurrent appends failed, first: %v", len(errs), writers, errs[0])
		}
		seen := map[uint64]bool{}
		for _, s := range seqs {
			if seen[s] {
				t.Fatalf("seq %d was assigned twice", s)
			}
			seen[s] = true
		}
		for want := uint64(1); want <= writers; want++ {
			if !seen[want] {
				t.Fatalf("seq %d was never assigned (got %v)", want, seqs)
			}
		}
		stored := db.Events().ForRun(run, 0, 1000)
		if len(stored) != writers {
			t.Fatalf("stored %d events, want %d: an append was lost", len(stored), writers)
		}
		for i, e := range stored {
			if e.Seq != uint64(i+1) {
				t.Fatalf("event %d has seq %d, want %d (the log must read back in seq order without gaps)", i, e.Seq, i+1)
			}
		}
	})

	t.Run("a replayed event id conflicts and consumes no seq", func(t *testing.T) {
		db := newDB(t)
		run := quarrycontracts.NewID(quarrycontracts.KindRun)
		first := newEvent(run)
		if seq, err := db.Events().AppendNext(first); err != nil || seq != 1 {
			t.Fatalf("first append = (%d, %v), want (1, nil)", seq, err)
		}
		if _, err := db.Events().AppendNext(first); !errors.Is(err, store.ErrConflict) {
			t.Fatalf("replayed event id = %v, want store.ErrConflict", err)
		}
		if seq, err := db.Events().AppendNext(newEvent(run)); err != nil || seq != 2 {
			t.Fatalf("next append = (%d, %v), want (2, nil): the replay must not use up a seq", seq, err)
		}
	})

	t.Run("each run is numbered on its own", func(t *testing.T) {
		db := newDB(t)
		a := quarrycontracts.NewID(quarrycontracts.KindRun)
		b := quarrycontracts.NewID(quarrycontracts.KindRun)
		for i := uint64(1); i <= 3; i++ {
			if seq, err := db.Events().AppendNext(newEvent(a)); err != nil || seq != i {
				t.Fatalf("run a append %d = (%d, %v)", i, seq, err)
			}
		}
		if seq, err := db.Events().AppendNext(newEvent(b)); err != nil || seq != 1 {
			t.Fatalf("run b first append = (%d, %v), want (1, nil)", seq, err)
		}
	})

	t.Run("an event with no run is seq 1", func(t *testing.T) {
		db := newDB(t)
		jid := quarrycontracts.NewID(quarrycontracts.KindJob)
		evt := quarrycontracts.Event{
			EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
			JobID:     &jid,
			Type:      quarrycontracts.EvtRunFailed,
			Timestamp: time.Now().UTC(),
		}
		if seq, err := db.Events().AppendNext(evt); err != nil || seq != 1 {
			t.Fatalf("append without a run = (%d, %v), want (1, nil)", seq, err)
		}
	})
}
