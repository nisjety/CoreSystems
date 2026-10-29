package sitesources

// Configured reads fresh protected deployment metadata only. It never contacts
// a provider, returns a key, or treats a different site's installation as access.
func (s *Service) Configured(org string, in Input) (bool, error) {
	if !orgPattern.MatchString(org) || !validInput(in) {
		return false, ErrInvalid
	}
	bindings, err := loadBindings(s.path)
	if err != nil {
		return false, ErrUnavailable
	}
	b, ok := bindings[bindingKey(org, in.WebsiteID)]
	return ok && b.Site == in.Site && b.WebsiteRevision == in.WebsiteRevision && (in.Provider == "cookiebot" && b.Cookiebot != nil || in.Provider == "algolia" && b.Algolia != nil), nil
}
