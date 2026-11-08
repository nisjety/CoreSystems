import os

path = os.path.normpath("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces/lib/space-approvals.test.ts")
b = open(path, "rb").read().decode("utf-8")
crlf = "\r\n" in b
t = b.replace("\r\n", "\n")
pairs = [
    (
        "  it('approving records the decision and resumes the run', async () => {\n",
        "  it('approving records the decision and never resumes the run from the client (WS10 S1)', async () => {\n",
    ),
    (
        "    expect(outcome).toBe('granted')\n    expect(orchestration.resumeRun).toHaveBeenCalledWith('run-1', undefined)\n    expect(orchestration.cancelRun).not.toHaveBeenCalled()\n",
        "    expect(outcome).toBe('granted')\n    expect(orchestration.resumeRun).not.toHaveBeenCalled()\n    expect(orchestration.cancelRun).not.toHaveBeenCalled()\n",
    ),
    (
        "  it('re-reads after an error and follows through when the decision did land', async () => {\n",
        "  it('re-reads after an error and reports the grant when the decision did land', async () => {\n",
    ),
    (
        "    expect(outcome).toBe('granted')\n    expect(orchestration.resumeRun).toHaveBeenCalledWith('run-1', undefined)\n  })\n",
        "    expect(outcome).toBe('granted')\n    expect(orchestration.resumeRun).not.toHaveBeenCalled()\n  })\n",
    ),
]
for old, new in pairs:
    assert t.count(old) == 1, (t.count(old), old[:70])
    t = t.replace(old, new)
if crlf:
    t = t.replace("\n", "\r\n")
open(path, "wb").write(t.encode("utf-8"))
print("space-approvals tests updated")
