//go:build linux || darwin || freebsd || netbsd || openbsd || dragonfly

package codexsubscription

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"time"
)

// lockCredentialFile takes an exclusive flock on the connection's lock file,
// polling without blocking so the wait stays bounded by ctx. It guards against
// a second broker process during an overlapping deploy; the in-process lock
// already serializes goroutines. A connection directory that does not exist
// has nothing to protect.
func lockCredentialFile(ctx context.Context, codeHome string) (func(), error) {
	file, err := os.OpenFile(filepath.Join(codeHome, credentialsLockName), os.O_RDWR|os.O_CREATE, 0o600)
	if errors.Is(err, os.ErrNotExist) {
		return func() {}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("open Codex credential lock: %w", err)
	}
	for {
		err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB)
		if err == nil {
			return func() {
				_ = syscall.Flock(int(file.Fd()), syscall.LOCK_UN)
				_ = file.Close()
			}, nil
		}
		if !errors.Is(err, syscall.EWOULDBLOCK) && !errors.Is(err, syscall.EINTR) {
			_ = file.Close()
			return nil, fmt.Errorf("lock Codex credentials: %w", err)
		}
		timer := time.NewTimer(credentialLockInterval)
		select {
		case <-ctx.Done():
			timer.Stop()
			_ = file.Close()
			return nil, ctx.Err()
		case <-timer.C:
		}
	}
}

// replaceFile atomically renames from over to.
func replaceFile(from, to string) error { return os.Rename(from, to) }

// syncDirectory makes a completed rename durable.
func syncDirectory(dir string) {
	handle, err := os.Open(dir)
	if err != nil {
		return
	}
	_ = handle.Sync()
	_ = handle.Close()
}
