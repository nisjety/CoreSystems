package grpc

import (
	"context"
	"fmt"
	"net"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
)

func TestServer_HealthCheckReturnsServing(t *testing.T) {
	port := freePort(t)
	server := NewServer(port)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	address := fmt.Sprintf("127.0.0.1:%d", port)
	waitForServing(t, address)

	server.Stop()

	select {
	case err := <-errCh:
		if err != nil {
			t.Fatalf("server returned error: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for gRPC server to stop")
	}
}

func waitForServing(t *testing.T, address string) {
	t.Helper()

	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		dialCtx, dialCancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
		conn, err := grpc.DialContext(
			dialCtx,
			address,
			grpc.WithTransportCredentials(insecure.NewCredentials()),
			grpc.WithBlock(),
		)
		dialCancel()
		if err == nil {
			client := healthpb.NewHealthClient(conn)
			checkCtx, checkCancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
			resp, checkErr := client.Check(checkCtx, &healthpb.HealthCheckRequest{})
			checkCancel()
			_ = conn.Close()
			if checkErr == nil && resp.GetStatus() == healthpb.HealthCheckResponse_SERVING {
				return
			}
		}
		time.Sleep(50 * time.Millisecond)
	}

	t.Fatalf("gRPC server on %s never reported SERVING", address)
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
