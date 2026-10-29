package sitesources

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"log"
	"time"
)

func newCollectionID(read func([]byte) (int, error)) (string, error) {
	var b [32]byte
	if n, err := read(b[:]); err != nil || n != len(b) || b == [32]byte{} {
		return "", ErrUnavailable
	}
	return hex.EncodeToString(b[:]), nil
}
func collectionID() (string, error) { return newCollectionID(rand.Read) }

// Caller holds the service mutex. The diagnostic contains no error, term or identity.
func (s *Service) saveCollectionsLocked(entries []collectionJob) error {
	err := s.collections.save(entries)
	s.collectionStorageFault = err != nil
	if err != nil && (s.collectionLastLog.IsZero() || s.now().Sub(s.collectionLastLog) >= time.Minute) {
		log.Printf("marketing-core: site source collection metadata storage unavailable")
		s.collectionLastLog = s.now()
	}
	return err
}
func (s *Service) Collection(ctx context.Context, id, site, provider string) (CollectionState, error) {
	if provider != "algolia" {
		return CollectionState{}, ErrInvalid
	}
	r, err := s.resolve(id, site, provider)
	if err != nil {
		return CollectionState{}, err
	}
	blank := collectionBlank(Enrollment{WebsiteID: id, Site: site, WebsiteRevision: r.Website.Revision})
	e, err := s.enrollment(id, site, r.Website.Revision)
	if err != nil {
		return blank, nil
	}
	a, err := s.collectionAccess(ctx, e)
	if err != nil {
		blank.Status = "needs_attention"
		blank.Reason = "provider_unavailable"
		return blank, nil
	}
	if !a.Active {
		blank.Status = "reconnect_required"
		blank.Reason = "reconnect_required"
		return blank, nil
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.collectionStorageFault {
		blank.Status = "needs_attention"
		blank.Reason = "provider_unavailable"
		return blank, nil
	}
	if s.collections != nil {
		for _, j := range s.collections.state.Entries {
			if sameEnrollment(jobEnrollment(j), e) {
				return j.CollectionState, nil
			}
		}
	}
	return blank, nil
}
func (s *Service) QueueCollection(id, site string, revision int64, provider string) (CollectionState, error) {
	if provider != "algolia" || revision < 1 || s.collections == nil {
		return CollectionState{}, ErrInvalid
	}
	e, err := s.enrollment(id, site, revision)
	if err != nil {
		return CollectionState{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	entries := append([]collectionJob{}, s.collections.state.Entries...)
	for i, j := range entries {
		if j.WebsiteID != id {
			continue
		}
		if sameEnrollment(jobEnrollment(j), e) && (j.Status == "queued" || j.Status == "syncing") {
			return j.CollectionState, nil
		}
		entries = append(entries[:i], entries[i+1:]...)
		break
	}
	if len(entries) >= 64 {
		return CollectionState{}, ErrCapacity
	}
	state := collectionBlank(e)
	state.Status = "queued"
	state.NextRunAt = stamp(s.now())
	runID, err := collectionID()
	if err != nil {
		return CollectionState{}, err
	}
	j := collectionJob{CollectionState: state, Fingerprint: e.Fingerprint, RunID: runID}
	entries = append(entries, j)
	if err = s.saveCollectionsLocked(entries); err != nil && !errors.Is(err, errCommitted) {
		return CollectionState{}, err
	}
	s.notifyCollection()
	return state, nil
}
func (s *Service) notifyCollection() {
	select {
	case s.collectionWake <- struct{}{}:
	default:
	}
}
func (s *Service) reconcileCollections() error {
	if s.collections == nil {
		return nil
	}
	active := map[string]Enrollment{}
	for _, e := range s.enrollments() {
		if s.currentEnrollment(e) {
			active[e.WebsiteID] = e
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	entries := make([]collectionJob, 0, len(active))
	if err := s.store.prune(s.now()); err != nil {
		s.collectionStorageFault = true
		if s.collectionLastLog.IsZero() || s.now().Sub(s.collectionLastLog) >= time.Minute {
			log.Printf("marketing-core: site source derived snapshot retention unavailable")
			s.collectionLastLog = s.now()
		}
		return err
	}
	seen := map[string]bool{}
	changed := false
	for _, j := range s.collections.state.Entries {
		e, ok := active[j.WebsiteID]
		if !ok || !sameEnrollment(jobEnrollment(j), e) {
			changed = true
			if cancel := s.collectionCancels[j.RunID]; cancel != nil {
				cancel()
			}
			continue
		}
		seen[j.WebsiteID] = true
		entries = append(entries, j)
	}
	for id, e := range active {
		if seen[id] {
			continue
		}
		state := collectionBlank(e)
		state.Status = "queued"
		state.NextRunAt = stamp(s.now())
		runID, err := collectionID()
		if err != nil {
			return err
		}
		entries = append(entries, collectionJob{CollectionState: state, Fingerprint: e.Fingerprint, RunID: runID})
		changed = true
	}
	if changed || s.collectionStorageFault {
		return s.saveCollectionsLocked(entries)
	}
	return nil
}
func (s *Service) claimCollection() (collectionJob, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.collections == nil {
		return collectionJob{}, false
	}
	entries := append([]collectionJob{}, s.collections.state.Entries...)
	for i, j := range entries {
		if j.Status == "syncing" || j.NextRunAt == "" || collectionTime(j.NextRunAt).After(s.now()) {
			continue
		}
		j.Status = "syncing"
		j.Reason = ""
		j.LastAttemptAt = stamp(s.now())
		j.NextRunAt = ""
		if j.Attempts < 1000 {
			j.Attempts++
		}
		entries[i] = j
		if err := s.saveCollectionsLocked(entries); err != nil && !errors.Is(err, errCommitted) {
			return collectionJob{}, false
		}
		return j, true
	}
	return collectionJob{}, false
}
func nextCollection(now time.Time, report Report) time.Time {
	next := collectionTime(report.ObservedAt).Add(Freshness)
	day := now.UTC()
	midnight := time.Date(day.Year(), day.Month(), day.Day()+1, 0, 0, 0, 0, time.UTC)
	if next.After(midnight) {
		next = midnight
	}
	if !next.After(now) {
		next = now.Add(time.Minute)
	}
	return next
}
