import type { SpaceThread } from '@/shared/api/spaces-client';
import { describe, expect, it } from 'vitest';

import { noTr, type Tr } from './space-copy';
import { narrateNewPosts, narrateRunChange, narrateSidebarMove, type NarrationContext } from './space-narration';

// WS16-S4: what a screen-reader user hears when the room changes under them. Every narrator is pure:
// it compares two listings and returns the announcements to make, so these tests need no DOM.

const en: Tr = (_no, english) => english;

const NAMES: Readonly<Record<string, string>> = { sub_kari: 'Kari', sub_ola: 'Ola' };
const context: NarrationContext = {
  roomName: 'Room A',
  nameOf: (subjectId) => (subjectId ? (NAMES[subjectId] ?? 'Member') : 'Member'),
};

function thread(overrides: Partial<SpaceThread> & Pick<SpaceThread, 'thread_id'>): SpaceThread {
  return { space_id: 'space_room_a', ...overrides };
}

describe('narrateNewPosts (WS16-S4)', () => {
  const viewer = 'sub_kari';
  const first = thread({ thread_id: 'thr_a', title: 'Plan', owner_subject_id: 'sub_kari', reply_count: 1 });
  const ola = thread({ thread_id: 'thr_b', title: 'Budget', owner_subject_id: 'sub_ola' });

  it('says nothing on a first listing, which is a baseline and not news', () => {
    expect(narrateNewPosts(undefined, [first, ola], viewer, noTr, context)).toEqual([]);
    expect(narrateNewPosts([], [first, ola], viewer, noTr, context)).toEqual([]);
  });

  it('never narrates a post the viewer wrote', () => {
    const mine = thread({ thread_id: 'thr_mine', title: 'Mine', owner_subject_id: 'sub_kari' });
    expect(narrateNewPosts([first], [first, mine], viewer, noTr, context)).toEqual([]);
  });

  it('says nothing about posts when the viewer is unknown', () => {
    expect(narrateNewPosts([first], [first, ola], undefined, noTr, context)).toEqual([]);
    expect(narrateNewPosts([first], [first, ola], '   ', noTr, context)).toEqual([]);
  });

  it('narrates a post by another member once, as a polite post in the room group', () => {
    const out = narrateNewPosts([first], [first, ola], viewer, noTr, context);
    expect(out).toHaveLength(1);
    expect(out[0]?.message).toBe('Nytt innlegg fra Ola: Budget');
    expect(out[0]?.options).toMatchObject({ politeness: 'polite', key: 'post:thr_b', kind: 'post', group: 'posts:Room A' });
    expect(out[0]?.options.summarise?.(2)).toBe('2 nye meldinger i Room A');
  });

  it('narrates a reply by another member, keyed by the reply count it reached', () => {
    const replied = thread({ ...first, reply_count: 2, last_author_subject_id: 'sub_ola' });
    const out = narrateNewPosts([first], [replied], viewer, noTr, context);
    expect(out.map((item) => item.message)).toEqual(['Ola svarte i Plan']);
    expect(out[0]?.options).toMatchObject({ politeness: 'polite', key: 'reply:thr_a:2', kind: 'post' });
  });

  it('does not narrate a reply the viewer wrote', () => {
    const mineReply = thread({ ...first, reply_count: 2, last_author_subject_id: 'sub_kari' });
    expect(narrateNewPosts([first], [mineReply], viewer, noTr, context)).toEqual([]);
  });

  it('speaks English when the caller translates to English', () => {
    const replied = thread({ ...first, reply_count: 2, last_author_subject_id: 'sub_ola' });
    expect(narrateNewPosts([first], [first, ola], viewer, en, context)[0]?.message).toBe('New post from Ola: Budget');
    expect(narrateNewPosts([first], [replied], viewer, en, context)[0]?.message).toBe('Ola replied in Plan');
    const out = narrateNewPosts([first], [first, ola], viewer, en, context);
    expect(out[0]?.options.summarise?.(2)).toBe('2 new messages in Room A');
  });
});

describe('narrateRunChange (WS16-S4)', () => {
  const running = thread({ thread_id: 'thr_a', title: 'Budget', latest_run_id: 'run_1', latest_run_status: 'running' });

  it('makes a run that needs approval assertive, keyed by its run', () => {
    const awaiting = { ...running, latest_run_status: 'awaiting_approval' };
    const out = narrateRunChange([running], [awaiting], noTr);
    expect(out).toHaveLength(1);
    expect(out[0]?.message).toBe('Venter på godkjenning: Budget');
    expect(out[0]?.options).toMatchObject({ politeness: 'assertive', key: 'approval:run_1', coalesceMs: 10_000 });
    expect(narrateRunChange([running], [awaiting], en)[0]?.message).toBe('Waiting for approval: Budget');
  });

  it('announces a failed run once, politely, as a status', () => {
    const failed = { ...running, latest_run_status: 'failed' };
    const out = narrateRunChange([running], [failed], noTr);
    expect(out).toHaveLength(1);
    expect(out[0]?.message).toBe('Kjøringen feilet: Budget');
    expect(out[0]?.options).toMatchObject({ politeness: 'polite', key: 'run:run_1:failed', kind: 'status' });
  });

  it('announces a completed run once, when it was active before', () => {
    const completed = { ...running, latest_run_status: 'completed' };
    const out = narrateRunChange([running], [completed], noTr);
    expect(out.map((item) => item.message)).toEqual(['Kjøringen er fullført: Budget']);
    expect(out[0]?.options).toMatchObject({ politeness: 'polite', key: 'run:run_1:completed', kind: 'status' });
  });

  it('does not announce a completion that was not preceded by an active run', () => {
    const failed = { ...running, latest_run_status: 'failed' };
    const completed = { ...running, latest_run_status: 'completed' };
    expect(narrateRunChange([failed], [completed], noTr)).toEqual([]);
  });

  it('says nothing when the listing has not changed, so a repeated listing does not repeat the announcement', () => {
    const failed = { ...running, latest_run_status: 'failed' };
    expect(narrateRunChange([failed], [{ ...failed }], noTr)).toEqual([]);
    expect(narrateRunChange([running], [{ ...running }], noTr)).toEqual([]);
  });

  it('announces a new run that fails under its own key', () => {
    const failedFirst = { ...running, latest_run_status: 'failed' };
    const failedSecond = { ...running, latest_run_id: 'run_2', latest_run_status: 'failed' };
    const out = narrateRunChange([failedFirst], [failedSecond], noTr);
    expect(out.map((item) => item.options.key)).toEqual(['run:run_2:failed']);
  });

  it('says nothing on a first listing, or for a thread that was not in the previous one', () => {
    const awaiting = { ...running, latest_run_status: 'awaiting_approval' };
    expect(narrateRunChange(undefined, [awaiting], noTr)).toEqual([]);
    const other = thread({ thread_id: 'thr_b', title: 'Other' });
    expect(narrateRunChange([other], [awaiting, other], noTr)).toEqual([]);
  });
});

describe('narrateSidebarMove (WS16-S4)', () => {
  it('says where the room moved, politely, as a status', () => {
    expect(narrateSidebarMove(noTr, 'Room A', 'Favoritter')).toEqual({
      message: 'Room A flyttet til Favoritter',
      options: { politeness: 'polite', kind: 'status' },
    });
    expect(narrateSidebarMove(en, 'Room A', 'Starred').message).toBe('Room A moved to Starred');
  });
});
