package publicsignals

func (s *Service) EnsureConnectionSync(id, site string, revision int64) error {
	r, err := s.binding(id, site, revision)
	if err != nil {
		return err
	}
	for _, e := range s.repo.read().Entries {
		if e.WebsiteID == id && e.Site == site && (!e.Enabled || matches(e, r.Website)) {
			return nil
		}
	}
	_, err = s.Queue(id, site, revision)
	return err
}
