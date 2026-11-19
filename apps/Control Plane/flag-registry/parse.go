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
// an error, so a typo cannot quietly read as off. A switch that sets OnLiteral or
// OnLiteralAnyCase in flag-registry.json is read by its literal instead, so this
// vocabulary does not apply to it.
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

// readsOn reports whether the canonical vocabulary reads value as on. An unknown value
// reads as neither.
func readsOn(value string) bool {
	on, err := ParseSwitch(value)
	return err == nil && on
}

// isBooleanLooking reports whether a compose default is a non-blank word in the canonical
// vocabulary, such as 0, 1, true, false or off. A blank default is an unset value, not a
// switch word, and a number such as 600 is not one either.
func isBooleanLooking(value string) bool {
	if strings.TrimSpace(value) == "" {
		return false
	}
	_, err := ParseSwitch(value)
	return err == nil
}
