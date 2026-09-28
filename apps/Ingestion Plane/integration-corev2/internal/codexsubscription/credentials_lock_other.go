//go:build !(linux || darwin || freebsd || netbsd || openbsd || dragonfly)

package codexsubscription

import (
	"context"
	"os"
	"time"
)

// lockCredentialFile is a no-op where flock is unavailable: the broker runs
// on Linux, and the in-process lock still serializes this process.
func lockCredentialFile(context.Context, string) (func(), error) { return func() {}, nil }

// replaceFile renames from over to. Windows refuses to replace a file another
// handle has open, so a concurrent reader is waited out briefly.
func replaceFile(from, to string) error {
	var err error
	for attempt := 0; attempt < 20; attempt++ {
		if err = os.Rename(from, to); err == nil {
			return nil
		}
		time.Sleep(10 * time.Millisecond)
	}
	return err
}

// syncDirectory is a no-op where a directory cannot be fsynced.
func syncDirectory(string) {}
