package analyticssync

// EnsureConnectionSync enrolls a missing current binding after consent. It
// preserves an explicit pause and never restarts a completed/current job. This
// also repairs a restart between saving new mappings and queuing collection.
func (s *Service) EnsureConnectionSync(id, site string, revision int64, q string) error {
	r, err := s.binding(id, site, q, revision)
	if err != nil {
		return err
	}
	for _, e := range s.repo.read().Entries {
		if e.WebsiteID == id && e.Site == site && e.Quarter == q && e.Year == s.now().UTC().Year() {
			if !e.Enabled || matches(e, r, q, s.now().UTC().Year()) {
				return nil
			}
		}
	}
	_, err = s.Queue(id, site, revision, q)
	return err
}
