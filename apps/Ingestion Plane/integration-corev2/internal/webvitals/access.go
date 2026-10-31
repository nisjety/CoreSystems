package webvitals

// Configured checks installation authority without fetching browser samples.
func (s *Service) Configured(org string, in Input) (bool, error) {
	if !orgPattern.MatchString(org) || !validInput(in) {
		return false, ErrInvalid
	}
	bindings, err := loadBindings(s.path, s.runtime)
	if err != nil {
		return false, ErrUnavailable
	}
	b, ok := bindings[bindingKey(org, in)]
	return ok && b.Site == in.Site && b.WebsiteRevision == in.WebsiteRevision, nil
}
