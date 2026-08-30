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
	"strconv"
	"time"
)

const (
	headerSig   = "X-Quarry-Sig"
	headerTS    = "X-Quarry-Sig-TS"
	headerNonce = "X-Quarry-Sig-Nonce"
)

// Sign adds the header triple verified by quarry-control. The caller supplies
// the exact request body bytes so the hash matches the bytes sent on the wire.
func Sign(req *http.Request, body []byte, secret string) error {
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
