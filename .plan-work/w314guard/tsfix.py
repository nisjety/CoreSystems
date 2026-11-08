import sys
sys.path.insert(0, r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\w314guard')
from edit import edit

edit('features/chat/components/ChatLiveRunPanel.tsx', [
("                          maxLength={GIVE_BACK_NOTE_MAX_CHARS}\n",
 "                          maxlength={GIVE_BACK_NOTE_MAX_CHARS}\n", 1),
])
edit('features/chat/components/WritePreview.tsx', [
("        aria-expanded={open()}\n", "        aria-expanded={open() ? 'true' : 'false'}\n", 1),
])
edit('features/chat/components/ChatMessages.approval-preview.test.tsx', [
("    fireEvent.click(disclosures()[0])\n    flush()\n    await waitFor(() => expect(screen.getByText(PREVIEW.bodyText)).toBeTruthy())\n    expect(mockGetApprovalPreview).toHaveBeenCalledTimes(1)\n    expect(mockGetApprovalPreview.mock.calls[0][0]).toBe('approval-mail')\n",
 "    fireEvent.click(disclosures()[0]!)\n    flush()\n    await waitFor(() => expect(screen.getByText(PREVIEW.bodyText)).toBeTruthy())\n    expect(mockGetApprovalPreview).toHaveBeenCalledTimes(1)\n    expect(mockGetApprovalPreview.mock.calls[0]?.[0]).toBe('approval-mail')\n", 1),
])
edit('shared/api/orchestration-client.test.ts', [
("    expect(String(fetchMock.mock.calls[0][0])).toContain('/api/v1/orchestration/approvals/appr%201/preview')\n",
 "    expect(String(fetchMock.mock.calls[0]?.[0])).toContain('/api/v1/orchestration/approvals/appr%201/preview')\n", 1),
])
