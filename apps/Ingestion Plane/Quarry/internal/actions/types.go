package actions

// ActionStep describes a single browser automation step.
type ActionStep struct {
	Type         string `json:"type"`
	Selector     string `json:"selector,omitempty"`
	Text         string `json:"text,omitempty"`
	Key          string `json:"key,omitempty"`
	Script       string `json:"script,omitempty"`       // for executeJavascript
	Milliseconds int    `json:"milliseconds,omitempty"` // for wait
	Direction    string `json:"direction,omitempty"`    // for scroll: "up" | "down"
	FullPage     bool   `json:"fullPage,omitempty"`
	Retry        int    `json:"retry,omitempty"`
	AfterShot    bool   `json:"screenshotAfter,omitempty"`
}

// ActionResult records the outcome of a single executed step.
type ActionResult struct {
	Type       string      `json:"type"`
	Success    bool        `json:"success"`
	DurationMs int64       `json:"durationMs"`
	Error      string      `json:"error,omitempty"`
	Output     interface{} `json:"output,omitempty"` // JS eval result or PDF bytes
}
