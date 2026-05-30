// Package notify provides email sending capabilities for transactional messages
// such as OTP codes. It supports Resend.com (preferred) and SMTP as a fallback.
package notify

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/smtp"
	"strings"
	"time"
)

// Sender is the interface implemented by email backends.
type Sender interface {
	SendOTP(to, otp string) error
}

// ResendSender uses the Resend.com HTTP API to deliver email.
type ResendSender struct {
	apiKey string
	from   string
	client *http.Client
}

// NewResendSender creates a ResendSender. from is the verified "From" address.
func NewResendSender(apiKey, from string) *ResendSender {
	return &ResendSender{
		apiKey: apiKey,
		from:   from,
		client: &http.Client{Timeout: 10 * time.Second},
	}
}

// SendOTP delivers a one-time code via Resend.
func (s *ResendSender) SendOTP(to, otp string) error {
	body := map[string]interface{}{
		"from":    s.from,
		"to":      []string{to},
		"subject": "Your verification code",
		"text":    fmt.Sprintf("Your one-time verification code is: %s\n\nIt expires in 10 minutes.", otp),
	}
	raw, err := json.Marshal(body)
	if err != nil {
		return fmt.Errorf("email: marshal resend payload: %w", err)
	}

	req, err := http.NewRequest(http.MethodPost, "https://api.resend.com/emails", bytes.NewReader(raw))
	if err != nil {
		return fmt.Errorf("email: build resend request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+s.apiKey)
	req.Header.Set("Content-Type", "application/json")

	resp, err := s.client.Do(req)
	if err != nil {
		return fmt.Errorf("email: resend request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 300 {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return fmt.Errorf("email: resend returned %d: %s", resp.StatusCode, strings.TrimSpace(string(b)))
	}
	return nil
}

// SMTPSender delivers email via SMTP with STARTTLS.
type SMTPSender struct {
	host string
	port int
	from string
	user string
	pass string
}

// NewSMTPSender creates an SMTPSender.
func NewSMTPSender(host string, port int, from, user, pass string) *SMTPSender {
	return &SMTPSender{host: host, port: port, from: from, user: user, pass: pass}
}

// SendOTP delivers a one-time code via SMTP.
func (s *SMTPSender) SendOTP(to, otp string) error {
	addr := fmt.Sprintf("%s:%d", s.host, s.port)
	auth := smtp.PlainAuth("", s.user, s.pass, s.host)

	msg := fmt.Sprintf(
		"From: %s\r\nTo: %s\r\nSubject: Your verification code\r\n\r\nYour one-time verification code is: %s\r\n\r\nIt expires in 10 minutes.",
		s.from, to, otp,
	)
	if err := smtp.SendMail(addr, auth, s.from, []string{to}, []byte(msg)); err != nil {
		return fmt.Errorf("email: smtp send: %w", err)
	}
	return nil
}

// NoopSender discards messages — used when no email backend is configured.
type NoopSender struct{}

func (NoopSender) SendOTP(to, otp string) error { return nil }
