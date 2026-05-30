package metrics

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestServer_ExposesPrometheusMetrics(t *testing.T) {
	port := freePort(t)
	server := NewServer(port)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	url := fmt.Sprintf("http://127.0.0.1:%d/metrics", port)
	waitForMetrics(t, url)

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		t.Fatalf("shutdown metrics server: %v", err)
	}

	select {
	case err := <-errCh:
		if err != nil {
			t.Fatalf("server returned error: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for metrics server to stop")
	}
}

func waitForMetrics(t *testing.T, url string) {
	t.Helper()

	client := &http.Client{Timeout: 300 * time.Millisecond}
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		resp, err := client.Get(url)
		if err == nil {
			body, readErr := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			if readErr == nil && resp.StatusCode == http.StatusOK && strings.Contains(string(body), "go_info") {
				return
			}
		}
		time.Sleep(50 * time.Millisecond)
	}

	t.Fatalf("metrics endpoint %s never became ready", url)
}

func freePort(t *testing.T) int {
	t.Helper()

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("allocate port: %v", err)
	}
	defer listener.Close()

	addr, ok := listener.Addr().(*net.TCPAddr)
	if !ok {
		t.Fatalf("unexpected listener address type: %T", listener.Addr())
	}

	return addr.Port
}
