package rawstore

import (
	"fmt"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// recordRefusalAt writes a download outcome that holds no body, as the engine does
// for a download that failed or was refused, stamped with the instant when.
func recordRefusalAt(t *testing.T, s *Store, run int64, id, outcome string, when time.Time) {
	t.Helper()
	if err := s.RecordAttempt(testCtx(t), run, doffinsync.Attempt{
		DoffinID: id, Outcome: outcome, Message: "refused", At: when,
	}); err != nil {
		t.Fatalf("RecordAttempt %s for %s: %v", outcome, id, err)
	}
}

// recordRefusal is recordRefusalAt at the fixed test instant.
func recordRefusal(t *testing.T, s *Store, run int64, id, outcome string) {
	t.Helper()
	recordRefusalAt(t, s, run, id, outcome, fixedNow)
}

// storeBody writes the held notice for id.
func storeBody(t *testing.T, s *Store, run int64, id string) {
	t.Helper()
	if _, err := s.StoreNotice(testCtx(t), run, guardNotice(id, []byte("held body of "+id))); err != nil {
		t.Fatalf("StoreNotice %s: %v", id, err)
	}
}

// at is the n-th hour after the fixed test instant. Each attempt of a history gets
// its own instant, so a test can tell which attempt a reported instant came from.
func at(n int) time.Time { return fixedNow.Add(time.Duration(n) * time.Hour) }

// rejectedWith is the Resolution of an id refused n times, the latest at instant last.
func rejectedWith(n int, last time.Time) doffinsync.Resolution {
	return doffinsync.Resolution{Kind: doffinsync.ResolutionRejected, Rejections: n, LastRejection: last}
}

var (
	heldResolution     = doffinsync.Resolution{Kind: doffinsync.ResolutionHeld}
	notFoundResolution = doffinsync.Resolution{Kind: doffinsync.ResolutionNotFound}
)

// assertResolutions fails unless got holds exactly the ids of want, each with the
// same kind, rejection count and last rejection instant. Instants are compared with
// Equal, so the time zone of a value read from the database does not matter.
func assertResolutions(t *testing.T, got, want map[string]doffinsync.Resolution) {
	t.Helper()
	if len(got) != len(want) {
		t.Errorf("Resolve returned %d ids, want %d:\n got  %+v\n want %+v", len(got), len(want), got, want)
	}
	for id, w := range want {
		g, ok := got[id]
		if !ok || g.Kind != w.Kind || g.Rejections != w.Rejections || !g.LastRejection.Equal(w.LastRejection) {
			t.Errorf("Resolve[%s] = %+v (present %v), want %+v", id, g, ok, w)
		}
	}
}

// S1 (R2, R3): an id whose latest definitive attempt was refused for its content is
// rejected. The store reports the facts, one refusal and its instant, and leaves the
// policy to the engine. An id with no attempt at all is absent.
func TestResolveReportsRejectedContent(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	recordRefusal(t, s, run, "2026-100021", doffinsync.OutcomeTooLarge)
	recordRefusal(t, s, run, "2026-100022", doffinsync.OutcomeEmptyBody)

	got, err := s.Resolve(ctx, []string{"2026-100021", "2026-100022", "2026-100023"})
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	assertResolutions(t, got, map[string]doffinsync.Resolution{
		"2026-100021": rejectedWith(1, fixedNow),
		"2026-100022": rejectedWith(1, fixedNow),
	})
}

// Precedence (R2): a held id beats everything. Otherwise the most recent definitive
// attempt decides between not found and rejected. The definitive outcomes are
// not_found, too_large and empty_body. A notice that returned 404 earlier but was
// refused for its body later is rejected, not not_found.
func TestResolvePrecedenceHeldThenLatestDefinitive(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	const (
		heldAfterNotFound    = "2026-100031" // a 404, then the body is stored: held wins
		heldAfterRefusal     = "2026-100032" // too large, then the body is stored: held wins
		heldBeforeRefusal    = "2026-100034" // the body is stored, then a later too large: held still wins
		refusedAfterNotFound = "2026-100033" // a 404, then a later too large: rejected
		notFoundAfterRefusal = "2026-100035" // too large, then a later 404: not found
	)
	recordRefusalAt(t, s, run, heldAfterNotFound, doffinsync.OutcomeNotFound, at(1))
	storeBody(t, s, run, heldAfterNotFound)
	recordRefusalAt(t, s, run, heldAfterRefusal, doffinsync.OutcomeTooLarge, at(1))
	storeBody(t, s, run, heldAfterRefusal)
	storeBody(t, s, run, heldBeforeRefusal)
	recordRefusalAt(t, s, run, heldBeforeRefusal, doffinsync.OutcomeTooLarge, at(2))
	recordRefusalAt(t, s, run, refusedAfterNotFound, doffinsync.OutcomeNotFound, at(1))
	recordRefusalAt(t, s, run, refusedAfterNotFound, doffinsync.OutcomeTooLarge, at(2))
	recordRefusalAt(t, s, run, notFoundAfterRefusal, doffinsync.OutcomeTooLarge, at(1))
	recordRefusalAt(t, s, run, notFoundAfterRefusal, doffinsync.OutcomeNotFound, at(2))

	got, err := s.Resolve(ctx, []string{heldAfterNotFound, heldAfterRefusal, heldBeforeRefusal, refusedAfterNotFound, notFoundAfterRefusal})
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	assertResolutions(t, got, map[string]doffinsync.Resolution{
		heldAfterNotFound:    heldResolution,
		heldAfterRefusal:     heldResolution,
		heldBeforeRefusal:    heldResolution,
		refusedAfterNotFound: rejectedWith(1, at(2)),
		notFoundAfterRefusal: notFoundResolution,
	})
}

// R2: rejected follows the latest definitive attempt, and a transient outcome after
// a refusal does not undo it. An id refused earlier and then answered by a transport
// error is still rejected; an id that failed earlier and was then refused is
// rejected, with the refusal's instant.
func TestResolveRejectedFollowsTheLatestDefinitiveAttempt(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	const (
		laterTransport = "2026-100041"
		laterRefusal   = "2026-100042"
	)
	recordRefusalAt(t, s, run, laterTransport, doffinsync.OutcomeTooLarge, at(1))
	recordRefusalAt(t, s, run, laterTransport, doffinsync.OutcomeTransportError, at(2))
	recordRefusalAt(t, s, run, laterRefusal, doffinsync.OutcomeTransportError, at(1))
	recordRefusalAt(t, s, run, laterRefusal, doffinsync.OutcomeTooLarge, at(2))

	got, err := s.Resolve(ctx, []string{laterTransport, laterRefusal})
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	assertResolutions(t, got, map[string]doffinsync.Resolution{
		laterTransport: rejectedWith(1, at(1)),
		laterRefusal:   rejectedWith(1, at(2)),
	})
}

// R2: the transient outcomes (http_error, transport_error, rate_limited,
// unauthorized, invalid_id) are ignored whenever the store decides. After nothing
// they leave the id absent. After a refusal or a 404 they leave that verdict, and the
// refusal's instant is the refusal's own, not the transient attempt's. Between two
// refusals they leave both counted.
func TestResolveIgnoresTransientOutcomes(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	transient := []string{
		doffinsync.OutcomeHTTPError, doffinsync.OutcomeTransportError, doffinsync.OutcomeRateLimited,
		doffinsync.OutcomeUnauthorized, doffinsync.OutcomeInvalidID,
	}
	want := make(map[string]doffinsync.Resolution)
	var all []string
	for i, outcome := range transient {
		base := 2000 + i*10
		only := fmt.Sprintf("2026-%06d", base+1)
		afterRefusal := fmt.Sprintf("2026-%06d", base+2)
		afterNotFound := fmt.Sprintf("2026-%06d", base+3)
		betweenRefusals := fmt.Sprintf("2026-%06d", base+4)
		all = append(all, only, afterRefusal, afterNotFound, betweenRefusals)

		recordRefusalAt(t, s, run, only, outcome, at(1))

		recordRefusalAt(t, s, run, afterRefusal, doffinsync.OutcomeTooLarge, at(1))
		recordRefusalAt(t, s, run, afterRefusal, outcome, at(2))
		want[afterRefusal] = rejectedWith(1, at(1))

		recordRefusalAt(t, s, run, afterNotFound, doffinsync.OutcomeNotFound, at(1))
		recordRefusalAt(t, s, run, afterNotFound, outcome, at(2))
		want[afterNotFound] = notFoundResolution

		recordRefusalAt(t, s, run, betweenRefusals, doffinsync.OutcomeTooLarge, at(1))
		recordRefusalAt(t, s, run, betweenRefusals, outcome, at(2))
		recordRefusalAt(t, s, run, betweenRefusals, doffinsync.OutcomeEmptyBody, at(3))
		want[betweenRefusals] = rejectedWith(2, at(3))
	}

	got, err := s.Resolve(ctx, all)
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	assertResolutions(t, got, want) // the ids with only a transient attempt are absent from want
}

// R3: Rejections counts every rejected attempt the id has ever had, whatever came
// between, and LastRejection is the instant of the latest one. They are only
// meaningful for a rejected id: a held id and a 404 carry neither.
func TestResolveCountsRejectionsAndReportsTheLatestOne(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	const (
		threeRefusals    = "2026-100051" // too large, empty body, too large
		refusalsAround   = "2026-100052" // too large, a 404, empty body: both refusals count
		notFoundLast     = "2026-100053" // two refusals, then a 404: not found, no count
		heldWithRefusals = "2026-100054" // two refusals, then the body: held, no count
		onceOnly         = "2026-100055"
	)
	recordRefusalAt(t, s, run, threeRefusals, doffinsync.OutcomeTooLarge, at(1))
	recordRefusalAt(t, s, run, threeRefusals, doffinsync.OutcomeEmptyBody, at(2))
	recordRefusalAt(t, s, run, threeRefusals, doffinsync.OutcomeTooLarge, at(3))

	recordRefusalAt(t, s, run, refusalsAround, doffinsync.OutcomeTooLarge, at(1))
	recordRefusalAt(t, s, run, refusalsAround, doffinsync.OutcomeNotFound, at(2))
	recordRefusalAt(t, s, run, refusalsAround, doffinsync.OutcomeEmptyBody, at(3))

	recordRefusalAt(t, s, run, notFoundLast, doffinsync.OutcomeTooLarge, at(1))
	recordRefusalAt(t, s, run, notFoundLast, doffinsync.OutcomeTooLarge, at(2))
	recordRefusalAt(t, s, run, notFoundLast, doffinsync.OutcomeNotFound, at(3))

	recordRefusalAt(t, s, run, heldWithRefusals, doffinsync.OutcomeTooLarge, at(1))
	recordRefusalAt(t, s, run, heldWithRefusals, doffinsync.OutcomeTooLarge, at(2))
	storeBody(t, s, run, heldWithRefusals)

	recordRefusalAt(t, s, run, onceOnly, doffinsync.OutcomeEmptyBody, at(5))

	got, err := s.Resolve(ctx, []string{threeRefusals, refusalsAround, notFoundLast, heldWithRefusals, onceOnly})
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	assertResolutions(t, got, map[string]doffinsync.Resolution{
		threeRefusals:    rejectedWith(3, at(3)),
		refusalsAround:   rejectedWith(2, at(3)),
		notFoundLast:     notFoundResolution,
		heldWithRefusals: heldResolution,
		onceOnly:         rejectedWith(1, at(5)),
	})
}
