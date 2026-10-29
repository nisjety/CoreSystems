package sitesources

import (
	"bytes"
	"encoding/json"
	"math"
	"strconv"
	"strings"
	"time"
)

func object(raw []byte) (map[string]json.RawMessage, bool) {
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return nil, false
	}
	return fields, true
}
func text(fields map[string]json.RawMessage, key string) (string, bool) {
	var value string
	raw, exists := fields[key]
	if !exists || bytes.Equal(raw, []byte("null")) || json.Unmarshal(raw, &value) != nil {
		return "", false
	}
	return value, true
}
func count(raw json.RawMessage) (int64, bool) {
	value := string(raw)
	if len(value) > 0 && value[0] == '"' {
		if json.Unmarshal(raw, &value) != nil {
			return 0, false
		}
	}
	if value == "" || len(value) > 16 || len(value) > 1 && value[0] == '0' {
		return 0, false
	}
	for _, c := range value {
		if c < '0' || c > '9' {
			return 0, false
		}
	}
	n, err := strconv.ParseInt(value, 10, 64)
	return n, err == nil && n <= safeMax
}
func bit(raw json.RawMessage) (int64, bool) { n, ok := count(raw); return n, ok && n <= 1 }
func dateInWindow(value string, window Window) bool {
	parsed, err := time.Parse(time.DateOnly, value)
	return err == nil && parsed.Format(time.DateOnly) == value && value >= window.StartDate && value <= window.EndDate
}
func consentDay(value string, window Window) (string, bool) {
	if dateInWindow(value, window) {
		return value, true
	}
	var parsed time.Time
	var err error
	if strings.HasSuffix(value, "Z") {
		parsed, err = time.Parse(time.RFC3339Nano, value)
	} else {
		parsed, err = time.Parse("2006-01-02T15:04:05", value)
	}
	if err != nil || parsed.Hour() != 0 || parsed.Minute() != 0 || parsed.Second() != 0 || parsed.Nanosecond() != 0 {
		return "", false
	}
	date := parsed.UTC().Format(time.DateOnly)
	return date, dateInWindow(date, window)
}
func boundary(value, expected string, end bool) bool {
	stamp, err := time.Parse(time.RFC3339Nano, value)
	if err != nil || !strings.HasSuffix(value, "Z") || stamp.UTC().Format(time.DateOnly) != expected {
		return false
	}
	if end {
		return stamp.Hour() == 23 && stamp.Minute() == 59 && stamp.Second() == 59
	}
	return stamp.Hour() == 0 && stamp.Minute() == 0 && stamp.Second() == 0 && stamp.Nanosecond() == 0
}
func add(total, value int64) (int64, bool) {
	if total > safeMax-value {
		return 0, false
	}
	return total + value, true
}
func ratio(raw json.RawMessage, numerator, denominator int64) bool {
	if denominator == 0 {
		if numerator != 0 {
			return false
		}
		if bytes.Equal(raw, []byte("null")) {
			return true
		}
		var value float64
		return len(raw) > 0 && json.Unmarshal(raw, &value) == nil && value == 0
	}
	var value float64
	if len(raw) == 0 || bytes.Equal(raw, []byte("null")) || json.Unmarshal(raw, &value) != nil || math.IsNaN(value) || math.IsInf(value, 0) || value < 0 || value > 1 {
		return false
	}
	return math.Abs(value-float64(numerator)/float64(denominator)) <= 1e-6
}
