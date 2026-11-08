import os

BASE = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces/components"
os.chdir(BASE)
path = "SpacePage.test.tsx"
b = open(path, "rb").read().decode("utf-8")
crlf = "\r\n" in b
t = b.replace("\r\n", "\n")


def sub(old, new):
    global t
    assert t.count(old) == 1, (t.count(old), old[:90])
    t = t.replace(old, new)


# 1. the room's approval decision goes through the Space route (it carries the scope); mock that module
sub(
    "const spacesClient = vi.hoisted(() => ({\n",
    "const spaceApprovals = vi.hoisted(() => ({ decideSpaceApproval: vi.fn() }))\n\nconst spacesClient = vi.hoisted(() => ({\n",
)
sub(
    "vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({\n",
    "vi.mock('@/shared/api/space-approvals-client', async (importOriginal) => ({\n"
    "  ...(await importOriginal<typeof import('@/shared/api/space-approvals-client')>()),\n"
    "  decideSpaceApproval: spaceApprovals.decideSpaceApproval,\n"
    "}))\n"
    "vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({\n",
)

# 2. the approval test: the Space route decides, and a granted approval never resumes the run from the client
sub(
    """    await waitFor(() => expect(orchestration.decideApproval).toHaveBeenCalled())
    expect(orchestration.decideApproval).toHaveBeenCalledWith('ap-1', 'approve', undefined, undefined)
    // Approving continues the run; the room then re-reads so the post stops
    // claiming it is waiting.
    await waitFor(() => expect(orchestration.resumeRun).toHaveBeenCalledWith('run_gate', undefined))
    await waitFor(() => expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(1))
""",
    """    // In a room the decision goes through the Space route, which carries the scope.
    await waitFor(() => expect(spaceApprovals.decideSpaceApproval).toHaveBeenCalled())
    expect(spaceApprovals.decideSpaceApproval).toHaveBeenCalledWith(
      'space_personal_1',
      'ap-1',
      expect.objectContaining({ decision: 'approve' }),
      undefined,
    )
    expect(orchestration.decideApproval).not.toHaveBeenCalled()
    // A granted approval never resumes the run from the client (WS10 S1); the room re-reads so the post
    // stops claiming it is waiting.
    expect(orchestration.resumeRun).not.toHaveBeenCalled()
    await waitFor(() => expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(1))
""",
)
sub(
    """    orchestration.listApprovals.mockResolvedValue([
      { id: 'ap-1', status: 'PENDING', detail: 'Send oppsummeringen til kunden' },
    ])
    renderSpacePage()
""",
    """    orchestration.listApprovals.mockResolvedValue([
      { id: 'ap-1', status: 'PENDING', detail: 'Send oppsummeringen til kunden' },
    ])
    spaceApprovals.decideSpaceApproval.mockResolvedValue({ approval: null, grant: { status: 'none' } })
    renderSpacePage()
""",
)

# 3. the pulse fills in once the thread list resolves: wait for it instead of reading the first paint
sub(
    """    expect(screen.getByText('Verevon jobber')).toBeTruthy()
""",
    """    expect(await screen.findByText('Verevon jobber')).toBeTruthy()
""",
)
sub(
    """    expect(document.querySelector('.verevon-space-pulse')?.getAttribute('data-attention')).toBe('true')
""",
    """    await waitFor(() =>
      expect(document.querySelector('.verevon-space-pulse')?.getAttribute('data-attention')).toBe('true'),
    )
""",
)
sub(
    """    expect(within(pulse).getByText('Samtaler')).toBeTruthy()
    // The live conversation is the card; the others follow newest first, never the live one twice.
    const titles = [...pulse.querySelectorAll('.verevon-space-pulse-recent__title')].map((node) => node.textContent)
    expect(titles).toEqual(['Nyere samtale', 'Eldre samtale'])
""",
    """    expect(await within(pulse).findByText('Samtaler')).toBeTruthy()
    // The live conversation is the card; the others follow newest first, never the live one twice.
    await waitFor(() => {
      const titles = [...pulse.querySelectorAll('.verevon-space-pulse-recent__title')].map((node) => node.textContent)
      expect(titles).toEqual(['Nyere samtale', 'Eldre samtale'])
    })
""",
)

# 4. marking the room read says why: entering it
sub(
    "    await waitFor(() => expect(spacesClient.markSpaceRead).toHaveBeenCalledWith('space_personal_1'))\n",
    "    await waitFor(() => expect(spacesClient.markSpaceRead).toHaveBeenCalledWith('space_personal_1', 'enter'))\n",
)

if crlf:
    t = t.replace("\n", "\r\n")
open(path, "wb").write(t.encode("utf-8"))
print("page tests updated")
