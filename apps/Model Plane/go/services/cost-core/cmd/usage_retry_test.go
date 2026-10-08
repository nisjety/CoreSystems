package main

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestUsageSubscriberRecoversAfterBrokerAndProvisionerStart(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	attempts := 0
	err := retryUsageConnection(ctx, time.Millisecond, func() error {
		attempts++
		if attempts == 1 {
			return errors.New("broker unavailable")
		}
		if attempts == 2 {
			return errors.New("consumer not provisioned")
		}
		return nil
	})
	if err != nil || attempts != 3 {
		t.Fatalf("attempts=%d error=%v", attempts, err)
	}
}

func TestUsageSubscriberShutdownCancelsRetryDelay(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	attempts := 0
	started := time.Now()
	err := retryUsageConnection(ctx, time.Hour, func() error {
		attempts++
		cancel()
		return errors.New("broker unavailable")
	})
	if !errors.Is(err, context.Canceled) || attempts != 1 || time.Since(started) > time.Second {
		t.Fatalf("shutdown failed: attempts=%d error=%v", attempts, err)
	}
	if err := retryUsageConnection(ctx, time.Millisecond, func() error { t.Fatal("retry after cancellation"); return nil }); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled subscriber did not stop")
	}
}
