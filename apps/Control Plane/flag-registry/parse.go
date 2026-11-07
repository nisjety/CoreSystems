package flagregistry

import (
	"errors"
	"fmt"
	"strings"
)

// ErrUnknownSwitchValue reports a switch value outside the canonical vocabulary.
var ErrUnknownSwitchValue = errors.New("unknown switch value")

// ParseSwitch reads a switch value in the canonical vocabulary. 1, true, on,
// yes and enabled turn a switch on. 0, false, off, no, disabled and the empty
// value turn it off. Case and surrounding space are ignored. Any other value is
// an error, so a typo cannot quietly read as off.
func ParseSwitch(raw string) (bool, error) {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "1", "true", "on", "yes", "enabled":
		return true, nil
	case "", "0", "false", "off", "no", "disabled":
		return false, nil
	default:
		return false, fmt.Errorf("%w: %q", ErrUnknownSwitchValue, raw)
	}
}
