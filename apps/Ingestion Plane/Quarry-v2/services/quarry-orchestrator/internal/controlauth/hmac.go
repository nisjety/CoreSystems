// Package controlauth signs orchestrator requests accepted by quarry-control's
// internal HMAC middleware.
package controlauth

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/rs/zerolog/log"
)

const (
	headerSig   = "X-Quarry-Sig"
	headerTS    = "X-Quarry-Sig-TS"
	headerNonce = "X-Quarry-Sig-Nonce"
)

// ControlInternalSecretEnv names the secret of the orchestrator-only control
// routes (D2-050). It is separate from QUARRY_INTERNAL_SECRET, which the edge also
// holds, so a leaked edge secret no longer reaches /v1/internal/jobs/*.
const ControlInternalSecretEnv = "QUARRY_CONTROL_INTERNAL_SECRET"

// getenv and warnf are variables so tests can replace them.
var (
	getenv = os.Getenv
	warnf  = func(msg string) { log.Warn().Msg(msg) }

	warnOnce sync.Once
)

// IsControlInternalPath reports whether path is one of the control routes only the
// orchestrator calls: everything under /v1/internal/jobs/, and the schedule routes
// GET /v1/internal/schedules/desired and POST /v1/internal/schedules/{id}/runs.
// POST /v1/internal/schedules/preflight is the edge's, not control's, and is not
// matched; neither are the edge's queue and run_page routes.
func IsControlInternalPath(path string) bool {
	if strings.HasPrefix(path, "/v1/internal/jobs/") {
		return true
	}
	rest, ok := strings.CutPrefix(path, "/v1/internal/schedules/")
	if !ok {
		return false
	}
	if rest == "desired" {
		return true
	}
	id, tail, found := strings.Cut(rest, "/")
	return found && id != "" && tail == "runs"
}

// secretFor picks the secret for one request. A control-internal route signs with
// QUARRY_CONTROL_INTERNAL_SECRET; when that is unset (or equal to the shared
// secret) it falls back to the shared secret so a deploy that has not set it keeps
// working, and logs one warning per process. Every other request signs with the
// shared secret passed by the caller.
func secretFor(req *http.Request, shared string) string {
	if req.URL == nil || !IsControlInternalPath(req.URL.Path) {
		return shared
	}
	own := getenv(ControlInternalSecretEnv)
	switch {
	case own == "":
		warnOnce.Do(func() {
			warnf(ControlInternalSecretEnv + " is unset: calls to control's orchestrator-only routes sign with QUARRY_INTERNAL_SECRET (shared with the edge). Set it; this fallback is removed after the soak (D2-050)")
		})
		return shared
	case own == shared:
		warnOnce.Do(func() {
			warnf(ControlInternalSecretEnv + " equals QUARRY_INTERNAL_SECRET: the orchestrator-only routes are not isolated from the edge (D2-050)")
		})
	}
	return own
}

// Sign adds the header triple verified by quarry-control. The caller supplies
// the exact request body bytes so the hash matches the bytes sent on the wire.
// shared is the secret the edge also holds; a request to one of control's
// orchestrator-only routes signs with the control-internal secret instead (see
// secretFor).
func Sign(req *http.Request, body []byte, shared string) error {
	secret := secretFor(req, shared)
	if secret == "" {
		return nil
	}
	nonceBytes := make([]byte, 16)
	if _, err := rand.Read(nonceBytes); err != nil {
		return fmt.Errorf("generate HMAC nonce: %w", err)
	}
	nonce := hex.EncodeToString(nonceBytes)
	ts := strconv.FormatInt(time.Now().Unix(), 10)
	bodyHash := sha256.Sum256(body)
	path := req.URL.EscapedPath()
	if req.URL.RawQuery != "" {
		path += "?" + req.URL.RawQuery
	}
	canonical := req.Method + "\n" + path + "\n" + hex.EncodeToString(bodyHash[:]) + "\n" + ts + "\n" + nonce
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	req.Header.Set(headerSig, "sig_v1="+base64.StdEncoding.EncodeToString(mac.Sum(nil)))
	req.Header.Set(headerTS, ts)
	req.Header.Set(headerNonce, nonce)
	return nil
}
