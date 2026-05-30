package driver

import "strings"

type SelectionInput struct {
	NeedJavaScript bool
	NeedScreenshot bool
	HasActions     bool
	WaitForMs      int
	Formats        []string
	Mobile         bool
	HasGeo         bool
	BlockAds       bool
	IsDocument     bool   // URL points to a PDF/DOCX/XLSX
	TargetURL      string // Used for auto-detection
}

type Selector struct {
	userAgent string
	stealth   bool
}

func NewSelector(userAgent string, stealth bool) *Selector {
	return &Selector{userAgent: userAgent, stealth: stealth}
}

func (s *Selector) Select(input SelectionInput) (PageDriver, error) {
	for _, format := range input.Formats {
		f := strings.ToLower(strings.TrimSpace(format))
		if f == "screenshot" {
			input.NeedScreenshot = true
			break
		}
		if f == "pdf" {
			input.NeedJavaScript = true // PDF generation requires Rod driver
			break
		}
	}
	if input.NeedJavaScript || input.NeedScreenshot || input.HasActions || input.WaitForMs > 0 || input.Mobile || input.HasGeo || input.BlockAds {
		return NewRodDriver(s.userAgent, s.stealth)
	}
	return NewCollyDriver(s.userAgent), nil
}
