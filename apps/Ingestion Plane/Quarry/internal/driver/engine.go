package driver

// Capability flags describe what a PageDriver can do.
type Capability uint32

const (
	CapHTTP       Capability = 1 << iota // Basic HTTP fetch
	CapJavaScript                        // JavaScript execution
	CapScreenshot                        // Full-page screenshot
	CapPDF                               // PDF generation
	CapActions                           // Click/Type/Scroll/Press
	CapMobile                            // Mobile device emulation
	CapGeo                               // Geolocation spoofing
	CapAdBlock                           // Ad / tracker blocking
	CapStealth                           // Anti-bot stealth mode
	CapDocument                          // PDF/DOCX/XLSX document ingestion
)

// EngineFactory creates a PageDriver lazily. This avoids launching browsers
// until the engine is actually selected by the waterfall.
type EngineFactory func() (PageDriver, error)

// Engine pairs a factory with the capabilities the resulting driver provides.
type Engine struct {
	Name         string
	Priority     int // Lower = tried first.
	Capabilities Capability
	Factory      EngineFactory
}

// Supports returns true if the engine provides every capability in the
// requested set.
func (e *Engine) Supports(required Capability) bool {
	return e.Capabilities&required == required
}
