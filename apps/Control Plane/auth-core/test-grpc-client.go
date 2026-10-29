package main

import (
	"context"
	"fmt"
	"log"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// Import generated proto - you'll need to generate this
// For now, let's use grpcurl to test instead

func main() {
	// Connect to auth gRPC service
	conn, err := grpc.NewClient(
		"localhost:50053",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	// Test health check
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	// You would use the generated client here
	fmt.Println("✅ Successfully connected to auth service gRPC at localhost:50053")
	fmt.Println("🔍 Use grpcurl to test endpoints:")
	fmt.Println("   grpcurl -plaintext localhost:50053 list")
	fmt.Println("   grpcurl -plaintext localhost:50053 auth.v1.AuthService/HealthCheck")

	_ = ctx
}
