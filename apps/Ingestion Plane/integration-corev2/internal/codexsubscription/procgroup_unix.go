//go:build unix

package codexsubscription

import (
	"errors"
	"os"
	"os/exec"
	"syscall"
)

// configureProcessGroup starts the app-server in its own process group, so
// Close can kill the Node shim, the native codex binary, and any MCP or
// `npm exec` children together.
func configureProcessGroup(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
}

// killProcessTree sends SIGKILL to the whole process group the command leads.
func killProcessTree(cmd *exec.Cmd) error {
	if cmd.Process == nil {
		return nil
	}
	err := syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	if err == nil {
		return nil
	}
	if errors.Is(err, syscall.ESRCH) {
		// The group is already gone; still make sure the leader is.
		if killErr := cmd.Process.Kill(); killErr != nil && !errors.Is(killErr, os.ErrProcessDone) {
			return killErr
		}
		return nil
	}
	if killErr := cmd.Process.Kill(); killErr != nil && !errors.Is(killErr, os.ErrProcessDone) {
		return killErr
	}
	return nil
}
