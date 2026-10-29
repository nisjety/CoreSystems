// Package cron provides a strict 5-field cron expression validator.
//
// Supported syntax (POSIX/Vixie-style subset):
//
//	field 1: minute     0-59
//	field 2: hour       0-23
//	field 3: dom        1-31
//	field 4: month      1-12
//	field 5: dow        0-6   (Sunday = 0)
//
// Per field, the accepted tokens are:
//   - "*"          wildcard
//   - "N"          single value within range
//   - "N-M"        range where N <= M
//   - "*/N"        step over wildcard, N > 0
//   - "N-M/S"      stepped range
//   - "a,b,c"      comma list of any of the above
//
// Unsupported (rejected): "L", "W", "?", "#", "@hourly"/"@daily"/etc. aliases,
// and the Quartz 6/7-field variants. Keep the grammar tight so schedules are
// deterministic across the control plane and Temporal scheduler.
package cron

import (
	"fmt"
	"strconv"
	"strings"
)

type fieldSpec struct {
	name     string
	min, max int
}

var fields = []fieldSpec{
	{"minute", 0, 59},
	{"hour", 0, 23},
	{"dom", 1, 31},
	{"month", 1, 12},
	{"dow", 0, 6},
}

// Validate returns nil if expr is a syntactically valid 5-field cron
// expression, or an error describing the first problem encountered.
func Validate(expr string) error {
	expr = strings.TrimSpace(expr)
	if expr == "" {
		return fmt.Errorf("cron: empty expression")
	}
	if strings.HasPrefix(expr, "@") {
		return fmt.Errorf("cron: aliases like %q are not supported", expr)
	}
	parts := strings.Fields(expr)
	if len(parts) != 5 {
		return fmt.Errorf("cron: expected 5 fields, got %d", len(parts))
	}
	for i, p := range parts {
		if err := validateField(p, fields[i]); err != nil {
			return err
		}
	}
	return nil
}

func validateField(f string, spec fieldSpec) error {
	if f == "" {
		return fmt.Errorf("cron: %s: empty field", spec.name)
	}
	for _, tok := range strings.Split(f, ",") {
		if err := validateToken(tok, spec); err != nil {
			return err
		}
	}
	return nil
}

func validateToken(tok string, spec fieldSpec) error {
	if tok == "" {
		return fmt.Errorf("cron: %s: empty list element", spec.name)
	}
	if strings.ContainsAny(tok, "LW?#") {
		return fmt.Errorf("cron: %s: unsupported syntax %q", spec.name, tok)
	}

	base := tok
	step := 0
	if idx := strings.Index(tok, "/"); idx >= 0 {
		base = tok[:idx]
		stepStr := tok[idx+1:]
		n, err := strconv.Atoi(stepStr)
		if err != nil || n <= 0 {
			return fmt.Errorf("cron: %s: invalid step %q", spec.name, stepStr)
		}
		step = n
	}

	if base == "*" {
		_ = step // wildcard with or without step is fine
		return nil
	}

	if idx := strings.Index(base, "-"); idx >= 0 {
		lo, err := parseInt(base[:idx], spec)
		if err != nil {
			return err
		}
		hi, err := parseInt(base[idx+1:], spec)
		if err != nil {
			return err
		}
		if lo > hi {
			return fmt.Errorf("cron: %s: range %d-%d is inverted", spec.name, lo, hi)
		}
		return nil
	}

	if step != 0 {
		return fmt.Errorf("cron: %s: step requires '*' or range, got %q", spec.name, base)
	}
	if _, err := parseInt(base, spec); err != nil {
		return err
	}
	return nil
}

func parseInt(s string, spec fieldSpec) (int, error) {
	n, err := strconv.Atoi(s)
	if err != nil {
		return 0, fmt.Errorf("cron: %s: not a number %q", spec.name, s)
	}
	if n < spec.min || n > spec.max {
		return 0, fmt.Errorf("cron: %s: %d out of range [%d,%d]", spec.name, n, spec.min, spec.max)
	}
	return n, nil
}
