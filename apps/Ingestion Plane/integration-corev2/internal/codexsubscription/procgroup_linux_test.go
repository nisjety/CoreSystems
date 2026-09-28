//go:build linux

package codexsubscription

import (
	"bufio"
	"errors"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
)

// processGone reports that pid no longer runs: it does not exist, or it is a
// zombie waiting for its (possibly non-reaping) parent.
func processGone(pid int) bool {
	if err := syscall.Kill(pid, 0); errors.Is(err, syscall.ESRCH) {
		return true
	}
	stat, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
	if err != nil {
		return true
	}
	// The state follows the parenthesised command name.
	fields := strings.Fields(string(stat[strings.LastIndexByte(string(stat), ')')+1:]))
	return len(fields) > 0 && (fields[0] == "Z" || fields[0] == "X")
}

func TestProcessGroupKilled(t *testing.T) {
	cmd := exec.Command("sh", "-c", "sleep 60 & echo $!; exec sleep 60")
	configureProcessGroup(cmd)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := cmd.Start(); err != nil {
		t.Skipf("sh unavailable: %v", err)
	}
	line, err := bufio.NewReader(stdout).ReadString('\n')
	if err != nil {
		t.Fatalf("read child pid: %v", err)
	}
	child, err := strconv.Atoi(strings.TrimSpace(line))
	if err != nil {
		t.Fatalf("child pid %q: %v", line, err)
	}
	leader := cmd.Process.Pid
	if err := killProcessTree(cmd); err != nil {
		t.Fatalf("killProcessTree: %v", err)
	}
	_ = cmd.Wait()
	deadline := time.Now().Add(2 * time.Second)
	for !(processGone(leader) && processGone(child)) {
		if time.Now().After(deadline) {
			t.Fatalf("processes still running: leader gone=%v child gone=%v", processGone(leader), processGone(child))
		}
		time.Sleep(20 * time.Millisecond)
	}
}
