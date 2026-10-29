package siteaudit

import "errors"

// At most two captures can finish without durable storage: new starts are
// blocked while these exact terminal snapshots await commit. They are never
// recaptured or replaced by an older passing result.
func (s *Service) retainTerminal(v Snapshot) {
	s.mu.Lock()
	defer s.mu.Unlock()
	next := map[string]Snapshot{}
	for id, old := range s.pendingTerminal {
		next[id] = copySnapshot(old)
	}
	next[v.ID] = copySnapshot(v)
	s.pendingTerminal = next
}

// Caller holds service.mu. A post-rename durability ambiguity already committed
// the terminal result and must not increment the attempt a second time.
func (s *Service) flushTerminal() error {
	next := map[string]Snapshot{}
	var failure error
	for id, v := range s.pendingTerminal {
		err := s.repo.save(v)
		if err != nil && !errors.Is(err, errCommitted) {
			next[id], failure = copySnapshot(v), err
		}
	}
	s.pendingTerminal = next
	return failure
}
