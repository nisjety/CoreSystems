import os

SRC = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces"


def edit(rel, pairs):
    path = os.path.join(SRC, rel)
    b = open(path, "rb").read().decode("utf-8")
    crlf = "\r\n" in b
    t = b.replace("\r\n", "\n")
    for old, new in pairs:
        assert t.count(old) == 1, (rel, t.count(old), old[:80])
        t = t.replace(old, new)
    if crlf:
        t = t.replace("\n", "\r\n")
    open(path, "wb").write(t.encode("utf-8"))


# The room decides through settleApproval with its scope, Space and grant callbacks; the copy says the
# run is not resumed and names the member who started it.
edit("components/SpaceApprovalPanel.test.tsx", [
    (
        """    expect(approvals.settleApproval).toHaveBeenCalledWith({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
    })
""",
        """    expect(approvals.settleApproval).toHaveBeenCalledWith(
      expect.objectContaining({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' }),
    )
""",
    ),
    (
        """    expect(await screen.findByText(/Kjøringen fortsetter/)).toBeTruthy()
""",
        """    // The copy never promises the run continues (WS10 S1): a grant says what it lets the member do.
    expect(await screen.findByText(/Godkjent\\. Handlingen kjøres/)).toBeTruthy()
""",
    ),
    (
        """    expect(await screen.findByText(/tilhører et annet medlem/)).toBeTruthy()
""",
        """    expect(await screen.findByText(/Venter på medlemmet som startet kjøringen/)).toBeTruthy()
""",
    ),
])

# A granted approval never resumes the run from the client (WS10 S1); the outcome is read from the server.
edit("lib/space-approvals.test.ts", [
    (
        """  it('approving records the decision and resumes the run', async () => {
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'GRANTED' })
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).toHaveBeenCalledWith('run-1', undefined)
""",
        """  it('approving records the decision and never resumes the run from the client (WS10 S1)', async () => {
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'GRANTED' })
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
""",
    ),
    (
        """  it('re-reads after an error and follows through when the decision did land', async () => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('gateway', 502, null))
    orchestration.listApprovals.mockResolvedValue([{ id: 'ap-1', status: 'GRANTED' }])
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).toHaveBeenCalledWith('run-1', undefined)
""",
        """  it('re-reads after an error and reports the grant when the decision did land', async () => {
    orchestration.decideApproval.mockRejectedValue(new ApiError('gateway', 502, null))
    orchestration.listApprovals.mockResolvedValue([{ id: 'ap-1', status: 'GRANTED' }])
    const outcome = await settleApproval({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve' })
    expect(outcome).toBe('granted')
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
""",
    ),
])
print("approval tests updated")
