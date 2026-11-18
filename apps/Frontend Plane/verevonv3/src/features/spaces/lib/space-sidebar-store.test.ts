import { emptySidebarDoc, type SidebarDoc } from '@/shared/actions/space-sidebar';
import { ApiError } from '@/shared/api/http';
import type { SpaceSidebarSaved, SpaceSidebarSnapshot } from '@/shared/api/spaces-sidebar-client';
import { LocalizedError } from '@/shared/i18n/errors';
import { flush } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { applySidebarAction } from './space-sidebar-organization';
import {
  createSpaceSidebarStore,
  SPACE_SIDEBAR_DEBOUNCE_MS,
  SPACE_SIDEBAR_STALE_AFTER_MS,
  spaceSidebarStorageKey,
  type SidebarStoreClient,
  type SpaceSidebarStore,
} from './space-sidebar-store';

// WS16-S9: the store behind the personal sidebar. The client is a fake, time is fake, and BroadcastChannel
// is a fake, so every path (cache, debounce, 409 rebase, 412, 404, cross-tab) runs deterministically with no
// network. Fixtures use neutral room names only.

const SCOPE = spaceSidebarStorageKey('org_1', 'user_1') as string;
const RETRY_MS = 5_000;

function starred(...refs: string[]): SidebarDoc {
  let doc = emptySidebarDoc();
  for (const ref of refs) doc = applySidebarAction(doc, { type: 'star', ref });
  return doc;
}

function fakeClient(snapshot: SpaceSidebarSnapshot = { mode: 'server', revision: 2, doc: null }) {
  return {
    get: vi.fn(async (): Promise<SpaceSidebarSnapshot> => snapshot),
    put: vi.fn(async (baseRevision: number, doc: SidebarDoc): Promise<SpaceSidebarSaved> => ({
      revision: baseRevision + 1,
      doc,
    })),
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

class FakeChannel {
  static instances: FakeChannel[] = [];
  readonly name: string;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  readonly posted: unknown[] = [];
  closed = false;

  constructor(name: string) {
    this.name = name;
    FakeChannel.instances.push(this);
  }

  postMessage(message: unknown) {
    this.posted.push(message);
  }

  close() {
    this.closed = true;
  }
}

const channel = (): FakeChannel => {
  const found = FakeChannel.instances[0];
  if (!found) throw new Error('the store did not open its channel');
  return found;
};

const opened: SpaceSidebarStore[] = [];
function openStore(client: SidebarStoreClient): SpaceSidebarStore {
  const store = createSpaceSidebarStore({ scopeKey: SCOPE, client });
  opened.push(store);
  return store;
}

/** Lets queued promises and timers run, then applies the signal writes the store made. */
async function settle(ms = 0): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  flush();
}

beforeEach(() => {
  vi.useFakeTimers();
  FakeChannel.instances = [];
  vi.stubGlobal('BroadcastChannel', FakeChannel);
});

afterEach(() => {
  for (const store of opened.splice(0)) store.dispose();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('createSpaceSidebarStore (WS16-S9)', () => {
  it('boots from the local cache at once, refuses edits until the GET answers, then takes the server layout', async () => {
    window.localStorage.setItem(SCOPE, JSON.stringify({ mode: 'server', revision: 3, doc: starred('room-cached') }));
    const client = fakeClient({ mode: 'server', revision: 5, doc: starred('room-server') });
    const store = openStore(client);
    flush();
    expect(store.state()).toMatchObject({ mode: 'loading', revision: 3, canEdit: false });
    expect(store.state().doc.starred).toEqual(['room-cached']);
    expect(() => store.dispatch({ type: 'star', ref: 'room-a' })).toThrow(LocalizedError);

    await settle();
    expect(client.get).toHaveBeenCalledTimes(1);
    expect(store.state()).toMatchObject({ mode: 'server', revision: 5, canEdit: true });
    expect(store.state().doc.starred).toEqual(['room-server']);
  });

  it('debounces edits into one PUT that carries the last revision the server returned', async () => {
    const client = fakeClient({ mode: 'server', revision: 4, doc: null });
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    await settle(100);
    store.dispatch({ type: 'star', ref: 'room-b' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS - 1);
    expect(client.put).not.toHaveBeenCalled();
    await settle(1);

    expect(client.put).toHaveBeenCalledTimes(1);
    const [base, sent] = client.put.mock.calls[0] as [number, SidebarDoc];
    expect(base).toBe(4);
    expect(sent.starred).toEqual(['room-a', 'room-b']);
    expect(channel().posted).toContainEqual({ scope: SCOPE, revision: 5 });

    store.dispatch({ type: 'star', ref: 'room-c' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    expect(client.put.mock.calls[1]?.[0]).toBe(5);
  });

  it('on a 409 rebases onto the stored layout and replays every action the server has not seen', async () => {
    const client = fakeClient({ mode: 'server', revision: 2, doc: null });
    const firstPut = deferred<SpaceSidebarSaved>();
    client.put.mockReturnValueOnce(firstPut.promise);
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    expect(client.put).toHaveBeenCalledTimes(1);

    // An edit made while the first PUT is in flight is not yet acknowledged, so it must be replayed too.
    store.dispatch({ type: 'star', ref: 'room-c' });
    firstPut.reject(
      new ApiError('conflict', 409, 'space_sidebar_conflict', { details: { revision: 5, doc: starred('room-b') } }),
    );
    await settle();

    expect(client.put).toHaveBeenCalledTimes(2);
    const [base, rebased] = client.put.mock.calls[1] as [number, SidebarDoc];
    expect(base).toBe(5);
    expect(rebased.starred).toEqual(['room-b', 'room-a', 'room-c']);
    flush();
    expect(store.state().doc.starred).toEqual(['room-b', 'room-a', 'room-c']);
  });

  it('counts a 409 whose stored document equals the one it sent as a success, not a conflict', async () => {
    const client = fakeClient({ mode: 'server', revision: 2, doc: null });
    client.put.mockRejectedValueOnce(
      new ApiError('conflict', 409, 'space_sidebar_conflict', { details: { revision: 3, doc: starred('room-a') } }),
    );
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    await settle(2 * RETRY_MS);

    expect(client.put).toHaveBeenCalledTimes(1);
    flush();
    expect(store.state()).toMatchObject({ status: 'idle', revision: 3, canEdit: true });
    expect(channel().posted).toContainEqual({ scope: SCOPE, revision: 3 });
  });

  it('keeps the edits after a failed PUT, makes them read-only, and retries them after five seconds', async () => {
    const client = fakeClient({ mode: 'server', revision: 2, doc: null });
    client.put.mockRejectedValueOnce(new ApiError('upstream unavailable', 503, 'space_sidebar_unavailable'));
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    expect(client.put).toHaveBeenCalledTimes(1);
    flush();
    expect(store.state()).toMatchObject({ status: 'error', canEdit: false });
    expect(store.state().doc.starred).toEqual(['room-a']);
    expect(() => store.dispatch({ type: 'star', ref: 'room-b' })).toThrow(LocalizedError);

    await settle(RETRY_MS);
    expect(client.put).toHaveBeenCalledTimes(2);
    expect(client.put.mock.calls[1]?.[0]).toBe(2);
    expect((client.put.mock.calls[1]?.[1] as SidebarDoc).starred).toEqual(['room-a']);
    flush();
    expect(store.state()).toMatchObject({ status: 'idle', canEdit: true });
  });

  it('a 412 switches to the browser-only layout, keeps it, and never PUTs again', async () => {
    const client = fakeClient({ mode: 'server', revision: 2, doc: null });
    client.put.mockRejectedValueOnce(new ApiError('zero retention', 412, 'zdr_durable_mutation_forbidden'));
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    flush();
    expect(store.state()).toMatchObject({ mode: 'zdr_local', browserOnly: true, canEdit: true, revision: 0 });
    expect(store.state().doc.starred).toEqual(['room-a']);

    store.dispatch({ type: 'star', ref: 'room-b' });
    await settle(10 * RETRY_MS);
    expect(client.put).toHaveBeenCalledTimes(1);
    flush();
    expect(store.state().doc.starred).toEqual(['room-a', 'room-b']);
  });

  it('a 404 on the PUT selects the disabled mode, refuses edits and never PUTs again', async () => {
    const client = fakeClient({ mode: 'server', revision: 2, doc: null });
    client.put.mockRejectedValueOnce(new ApiError('sidebar off', 404, 'space_sidebar_disabled'));
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    flush();
    expect(store.state()).toMatchObject({ mode: 'disabled', canEdit: false });
    expect(() => store.dispatch({ type: 'star', ref: 'room-b' })).toThrow(LocalizedError);
    await settle(10 * RETRY_MS);
    expect(client.put).toHaveBeenCalledTimes(1);
  });

  it('a 404 on the GET selects the disabled mode straight away, with no PUT', async () => {
    const client = fakeClient();
    client.get.mockRejectedValueOnce(new ApiError('sidebar off', 404, 'space_sidebar_disabled'));
    const store = openStore(client);
    await settle();
    expect(store.state()).toMatchObject({ mode: 'disabled', canEdit: false });
    expect(client.put).not.toHaveBeenCalled();
  });

  it('zdr_local never PUTs, and the only browser write is the layout under its own key', async () => {
    const client = fakeClient({ mode: 'zdr_local', revision: 0, doc: null });
    const store = openStore(client);
    await settle();
    flush();
    expect(store.state()).toMatchObject({ mode: 'zdr_local', browserOnly: true, canEdit: true });

    store.dispatch({ type: 'star', ref: 'room-a' });
    store.dispatch({ type: 'section.create', id: 's1', name: 'Work' });
    await settle(10 * RETRY_MS);
    channel().onmessage?.({ data: { scope: SCOPE, revision: 7 } });
    await settle(SPACE_SIDEBAR_STALE_AFTER_MS);

    expect(client.put).not.toHaveBeenCalled();
    expect(client.get).toHaveBeenCalledTimes(1);
    expect(window.localStorage.length).toBe(1);
    expect(window.localStorage.key(0)).toBe(SCOPE);
    flush();
    const stored = JSON.parse(window.localStorage.getItem(SCOPE) ?? 'null') as { mode: string; doc: unknown };
    expect(stored.mode).toBe('zdr_local');
    expect(stored.doc).toEqual(store.state().doc);
  });

  it('refetches when another tab reports a newer revision of the same layout, and ignores other layouts', async () => {
    const client = fakeClient({ mode: 'server', revision: 3, doc: null });
    openStore(client);
    await settle();
    expect(client.get).toHaveBeenCalledTimes(1);

    channel().onmessage?.({ data: { scope: SCOPE, revision: 3 } });
    await settle();
    channel().onmessage?.({ data: { scope: 'verevon.space-sidebar.v1:org_2:user_9', revision: 9 } });
    await settle();
    expect(client.get).toHaveBeenCalledTimes(1);

    channel().onmessage?.({ data: { scope: SCOPE, revision: 9 } });
    await settle();
    expect(client.get).toHaveBeenCalledTimes(2);
  });

  it('does not refetch over an edit that is still waiting to be saved', async () => {
    const client = fakeClient({ mode: 'server', revision: 3, doc: null });
    const store = openStore(client);
    await settle();

    store.dispatch({ type: 'star', ref: 'room-a' });
    channel().onmessage?.({ data: { scope: SCOPE, revision: 9 } });
    await settle();
    expect(client.get).toHaveBeenCalledTimes(1);

    await settle(SPACE_SIDEBAR_DEBOUNCE_MS);
    expect(client.put).toHaveBeenCalledTimes(1);
  });
});
