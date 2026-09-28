//go:build !unix

package codexsubscription

import (
	"errors"
	"os"
	"os/exec"
)

// configureProcessGroup is a no-op where process groups are unavailable.
func configureProcessGroup(*exec.Cmd) {}

// killProcessTree kills the app-server process itself.
func killProcessTree(cmd *exec.Cmd) error {
	if cmd.Process == nil {
		return nil
	}
	if err := cmd.Process.Kill(); err != nil && !errors.Is(err, os.ErrProcessDone) {
		return err
	}
	return nil
}
