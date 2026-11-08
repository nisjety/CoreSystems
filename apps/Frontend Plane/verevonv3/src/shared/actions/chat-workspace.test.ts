import { describe, expect, it } from 'vitest'
import {
  parseChatWorkspaceAction,
  parseThreadWorkspaceListing,
  parseThreadWorkspaceReset,
  threadWorkspacePath,
} from '@/shared/actions/chat-workspace'

describe('chat workspace action contract', () => {
  it('accepts the list and a reset the caller has confirmed', () => {
    expect(parseChatWorkspaceAction({ type: 'chat.workspace.list', threadId: 'thr_01J9' })).toEqual({
      type: 'chat.workspace.list',
      threadId: 'thr_01J9',
    })
    expect(parseChatWorkspaceAction({ type: 'chat.workspace.reset', threadId: 'thr_01J9', confirm: true })).toEqual({
      type: 'chat.workspace.reset',
      threadId: 'thr_01J9',
      confirm: true,
    })
  })

  // Erasure is never a side effect of a click that did not say so.
  it('refuses a reset that is not confirmed', () => {
    expect(() => parseChatWorkspaceAction({ type: 'chat.workspace.reset', threadId: 'thr_1' })).toThrow()
    expect(() => parseChatWorkspaceAction({ type: 'chat.workspace.reset', threadId: 'thr_1', confirm: false })).toThrow()
  })

  it('refuses a thread id that is not a plain key segment', () => {
    for (const bad of ['', 'thr/1', '../thr', 'thr 1', 'thr.1', 'a'.repeat(129)]) {
      expect(() => parseChatWorkspaceAction({ type: 'chat.workspace.list', threadId: bad })).toThrow()
    }
  })

  it('reads the listing the BFF returns into camel case', () => {
    expect(
      parseThreadWorkspaceListing({
        thread_id: 'thr_1',
        revision: 3,
        files: [{ path: 'report.csv', content_hash: `sha256:${'c'.repeat(64)}`, size_bytes: 2048 }],
      }),
    ).toEqual({
      threadId: 'thr_1',
      revision: 3,
      files: [{ path: 'report.csv', contentHash: `sha256:${'c'.repeat(64)}`, sizeBytes: 2048 }],
    })
  })

  it('refuses a listing that is not the BFF shape', () => {
    expect(() => parseThreadWorkspaceListing({ thread_id: 'thr_1', revision: -1, files: [] })).toThrow()
    expect(() => parseThreadWorkspaceListing({ thread_id: 'thr_1', revision: 0, files: [{ path: 'a' }] })).toThrow()
  })

  it('reads the reset receipt into camel case', () => {
    expect(
      parseThreadWorkspaceReset({ thread_id: 'thr_1', files_deleted: 4, object_prefix: 'cas/thread/org-1/thr_1/' }),
    ).toEqual({ threadId: 'thr_1', filesDeleted: 4 })
  })

  it('builds the BFF path for the thread, with the id checked first', () => {
    expect(threadWorkspacePath('thr_01J9')).toBe('/api/v1/chat/threads/thr_01J9/workspace')
    expect(() => threadWorkspacePath('thr/01J9')).toThrow()
  })
})
