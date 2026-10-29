package config

import (
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// CrUX uses a dedicated, optional protected file. PageSpeed keys are not
// silently reused: they may be restricted to a different Google API.
func loadCrUXKey() (string, error) {
	invalid := errors.New("CrUX requires a protected valid CRUX_API_KEY_FILE")
	if strings.TrimSpace(os.Getenv("CRUX_API_KEY")) != "" {
		return "", invalid
	}
	path := strings.TrimSpace(os.Getenv("CRUX_API_KEY_FILE"))
	if path == "" {
		return "", nil
	}
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return "", invalid
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() < 1 || info.Size() > 4096 {
		return "", invalid
	}
	f, err := os.Open(path)
	if err != nil {
		return "", invalid
	}
	defer f.Close()
	current, err := f.Stat()
	if err != nil || !os.SameFile(info, current) || current.Mode().Perm() != 0600 {
		return "", invalid
	}
	raw, err := io.ReadAll(io.LimitReader(f, 4097))
	if err != nil || len(raw) > 4096 {
		return "", invalid
	}
	key := strings.TrimSpace(string(raw))
	if len(key) < 1 || len(key) > 256 {
		return "", invalid
	}
	for _, char := range key {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '_' || char == '-') {
			return "", invalid
		}
	}
	return key, nil
}
