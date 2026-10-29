package websites

import (
	"context"
	"regexp"
)

var connectionsFingerprintPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)

// RefreshConnections reconciles only sites with changed Integration authority.
// The checkpoint is private, durable, and written with the verified mapping in
// the existing registry transaction. A failure remains eligible for retry.
func (s *Service) RefreshConnections(ctx context.Context, fingerprint string, changed func(Website)) error {
	if !connectionsFingerprintPattern.MatchString(fingerprint) {
		return ErrInvalid
	}
	for _, r := range s.repo.List() {
		if err := ctx.Err(); err != nil {
			return err
		}
		if r.ConnectionsFingerprint == fingerprint {
			if changed != nil {
				changed(r.Website)
			}
			continue
		}
		w, err := s.Verify(ctx, r.Website.ID)
		if err != nil {
			continue
		}
		current, err := s.Resolve(w.ID, w.Site)
		if err == nil && current.ConnectionsFingerprint != "" && current.ConnectionsFingerprint != r.ConnectionsFingerprint && changed != nil {
			changed(current.Website)
		}
	}
	return nil
}
