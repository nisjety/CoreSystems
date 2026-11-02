package api

import (
	"bufio"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
)

// A request the HTTP parser refuses is answered with the text of the framework's error, and the server puts a piece of
// the raw request in that text: the first and the last two hundred bytes of the head. Behind the gateway the head holds
// the key the gateway adds to what it passes on, the tenant and the user, and the gateway passes the answer on to
// whoever sent the request. Any header the parser refuses is enough to get it back.
func TestARequestTheParserRefusesIsNotAnsweredWithAPieceOfItself(t *testing.T) {
	t.Parallel()

	head := "POST /api/v1/sources HTTP/1.1\r\nHost: anbud\r\nX-API-Key: " + serverKey + "\r\nX-Org-ID: org-a\r\nX-User-ID: user-1\r\n"
	for name, extra := range map[string]string{
		"a trailer that names a header the server owns": "Trailer: Host\r\n",
		"a trailer that names the content type":         "Trailer: Content-Type\r\n",
		"two lengths":                                   "Content-Length: 1\r\nContent-Length: 1\r\n",
		"a transfer coding it does not know":            "Transfer-Encoding: gzip\r\n",
		"a control character in a value":                "X-Note: a\x7fb\r\n",
		"a header line with no colon":                   "Broken\r\n",
	} {
		docs := &fakeDocuments{}
		app, logs := docsServer(t, docs)
		status, body := rawRequest(t, app, head+extra+"\r\n")
		if status != http.StatusBadRequest {
			t.Errorf("%s: status %d, want 400: %s", name, status, body)
			continue
		}
		for _, piece := range []string{serverKey, "org-a", "user-1", "X-API-Key", "Buffer size", "contents"} {
			if strings.Contains(body, piece) {
				t.Errorf("%s: the answer holds %q, a piece of the request: %s", name, piece, body)
			}
		}
		if e := decodeError(t, body); e.Error.Code != "bad_request" {
			t.Errorf("%s: code %q, want bad_request", name, e.Error.Code)
		}
		if strings.Contains(logs.String(), serverKey) {
			t.Errorf("%s: the key is in the log", name)
		}
	}
}

// What a client is told of an error the framework made is fixed by its status, and is never the framework's own text.
func TestWhatAClientIsToldOfAFrameworkErrorIsFixedByItsStatus(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		status  int
		message string
	}{
		{fiber.StatusBadRequest, "the request could not be read"},
		{fiber.StatusNotFound, "route not found"},
		{fiber.StatusMethodNotAllowed, http.StatusText(http.StatusMethodNotAllowed)},
		{fiber.StatusRequestTimeout, http.StatusText(http.StatusRequestTimeout)},
		{fiber.StatusRequestEntityTooLarge, http.StatusText(http.StatusRequestEntityTooLarge)},
		{fiber.StatusRequestHeaderFieldsTooLarge, http.StatusText(http.StatusRequestHeaderFieldsTooLarge)},
		{599, "error"}, // a status nobody has named
	} {
		if got := publicMessage(tc.status); got != tc.message || got == "" {
			t.Errorf("status %d: %q, want %q", tc.status, got, tc.message)
		}
	}

	app, _ := docsServer(t, &fakeDocuments{})
	// A message the framework made up, with a piece of a request in it, comes out as the fixed one.
	status, body := rawRequest(t, app, "POST /api/v1/sources HTTP/1.1\r\nHost: anbud\r\nX-API-Key: "+serverKey+"\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n")
	if status != http.StatusBadRequest || decodeError(t, body).Error.Message != "the request could not be read" {
		t.Errorf("a refused request: %d %s", status, body)
	}
}

// The error handler is the first layer, and this is the second, tried with the first out of the way: the error the
// server itself ends the connection with, which is what a logger set later would print, holds no piece of the request
// either.
func TestTheServersOwnErrorForARefusedRequestHoldsNoPieceOfIt(t *testing.T) {
	t.Parallel()

	app, _ := docsServer(t, &fakeDocuments{})
	_ = app.Handler() // builds the routes, which ServeConn does not
	client, server := net.Pipe()
	defer client.Close()
	served := make(chan error, 1)
	go func() { served <- app.Server().ServeConn(server) }()
	_ = client.SetDeadline(time.Now().Add(20 * time.Second))
	raw := "POST /api/v1/sources HTTP/1.1\r\nHost: anbud\r\nX-API-Key: " + serverKey + "\r\nX-Org-ID: org-a\r\nTrailer: Host\r\n\r\n"
	if _, err := client.Write([]byte(raw)); err != nil {
		t.Fatalf("write: %v", err)
	}
	resp, err := http.ReadResponse(bufio.NewReader(client), nil)
	if err != nil {
		t.Fatalf("read the response: %v", err)
	}
	_ = resp.Body.Close()
	_ = client.Close()

	select {
	case err := <-served:
		if err == nil {
			t.Fatal("the server ended the connection with no error: the request was not refused by the parser")
		}
		for _, piece := range []string{serverKey, "org-a", "contents"} {
			if strings.Contains(err.Error(), piece) {
				t.Errorf("the server's error holds %q, a piece of the request: %v", piece, err)
			}
		}
	case <-time.After(20 * time.Second):
		t.Fatal("the connection was not ended")
	}
}
