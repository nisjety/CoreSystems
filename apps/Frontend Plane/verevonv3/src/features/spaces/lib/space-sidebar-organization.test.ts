import limits from '@/shared/actions/space-sidebar-limits.json';
import { emptySidebarDoc, SIDEBAR_LIMITS, type SidebarDoc, type SidebarSection } from '@/shared/actions/space-sidebar';
import type { SpaceSummary } from '@/shared/api/spaces-client';
import { LocalizedError } from '@/shared/i18n/errors';
import { describe, expect, it } from 'vitest';

import {
  applySidebarAction,
  buildSidebarModel,
  pruneDoc,
  type SidebarGroupSpec,
  type SidebarModel,
  type SidebarModelInput,
  type SidebarRoomAttention,
} from './space-sidebar-organization';

// WS16-S9 and WS16-S10: the personal Spaces sidebar. The reducer is the one gate every edit goes through,
// and the model is the slot table the panel draws. Fixtures use neutral names only.

const room = (ref: string, name = ref, kind = 'channel'): SpaceSummary => ({ space_ref: ref, name, kind, lifecycle: 'active' });

const GROUPS: SidebarGroupSpec[] = [
  { id: 'channels', match: (space) => space.kind === 'channel' },
  { id: 'dms', match: (space) => space.kind === 'dm' },
];

const docOf = (patch: Partial<SidebarDoc>): SidebarDoc => ({ ...emptySidebarDoc(), ...patch });

const section = (id: string, name: string, refs: string[], sort: SidebarSection['sort'] = 'manual'): SidebarSection => ({
  id,
  name,
  collapsed: false,
  sort,
  refs,
});

function modelOf(spaces: SpaceSummary[], doc: SidebarDoc, extra: Partial<SidebarModelInput> = {}): SidebarModel {
  return buildSidebarModel({
    spaces,
    doc,
    groups: GROUPS,
    query: '',
    matches: (space, query) => space.name.toLowerCase().includes(query),
    displayName: (space) => space.name,
    ...extra,
  });
}

const refsOf = (rooms: readonly SpaceSummary[]) => rooms.map((space) => space.space_ref);
const sectionRefs = (model: SidebarModel, id: string) =>
  refsOf(model.sections.find((entry) => entry.section.id === id)?.rooms ?? []);
const groupRefs = (model: SidebarModel, id: string) => refsOf(model.groups.find((group) => group.id === id)?.rooms ?? []);

describe('applySidebarAction (WS16-S9)', () => {
  it('takes every cap from the shared limits file', () => {
    expect(SIDEBAR_LIMITS).toEqual({
      maxBytes: limits.maxBytes,
      starred: limits.starred,
      hidden: limits.hidden,
      sections: limits.sections,
      refsPerSection: limits.refsPerSection,
      name: limits.name,
    });
  });

  it('keeps Starred and Hidden apart: starring unhides a room, hiding unstars it', () => {
    const hidden = applySidebarAction(emptySidebarDoc(), { type: 'room.hide', ref: 'room-a' });
    expect(hidden.hidden).toEqual(['room-a']);
    const starred = applySidebarAction(hidden, { type: 'star', ref: 'room-a' });
    expect(starred.starred).toEqual(['room-a']);
    expect(starred.hidden).toEqual([]);
    const hiddenAgain = applySidebarAction(starred, { type: 'room.hide', ref: 'room-a' });
    expect(hiddenAgain.starred).toEqual([]);
    expect(hiddenAgain.hidden).toEqual(['room-a']);
  });

  it('returns the same document for a star or a hide that changes nothing', () => {
    const starred = applySidebarAction(emptySidebarDoc(), { type: 'star', ref: 'room-a' });
    expect(applySidebarAction(starred, { type: 'star', ref: 'room-a' })).toBe(starred);
    const hidden = applySidebarAction(emptySidebarDoc(), { type: 'room.hide', ref: 'room-h' });
    expect(applySidebarAction(hidden, { type: 'room.hide', ref: 'room-h' })).toBe(hidden);
  });

  it('unstars a room and moves a starred room before another or to the end', () => {
    let doc = emptySidebarDoc();
    for (const ref of ['room-a', 'room-b', 'room-c']) doc = applySidebarAction(doc, { type: 'star', ref });
    expect(applySidebarAction(doc, { type: 'unstar', ref: 'room-b' }).starred).toEqual(['room-a', 'room-c']);
    expect(applySidebarAction(doc, { type: 'star.move', ref: 'room-c', beforeRef: 'room-a' }).starred).toEqual([
      'room-c',
      'room-a',
      'room-b',
    ]);
    expect(applySidebarAction(doc, { type: 'star.move', ref: 'room-a', beforeRef: null }).starred).toEqual([
      'room-b',
      'room-c',
      'room-a',
    ]);
  });

  it('caps starred rooms, hidden rooms, sections and rooms per section at the shared limits', () => {
    let starred = emptySidebarDoc();
    for (let i = 0; i < limits.starred; i += 1) starred = applySidebarAction(starred, { type: 'star', ref: `room-${i}` });
    expect(starred.starred).toHaveLength(limits.starred);
    expect(() => applySidebarAction(starred, { type: 'star', ref: 'room-over' })).toThrow(
      `You can star up to ${limits.starred} rooms.`,
    );

    let hidden = emptySidebarDoc();
    for (let i = 0; i < limits.hidden; i += 1) hidden = applySidebarAction(hidden, { type: 'room.hide', ref: `room-${i}` });
    expect(hidden.hidden).toHaveLength(limits.hidden);
    expect(() => applySidebarAction(hidden, { type: 'room.hide', ref: 'room-over' })).toThrow(LocalizedError);

    let sections = emptySidebarDoc();
    for (let i = 0; i < limits.sections; i += 1) {
      sections = applySidebarAction(sections, { type: 'section.create', id: `s${i}`, name: `Group ${i}` });
    }
    expect(sections.sections).toHaveLength(limits.sections);
    expect(() => applySidebarAction(sections, { type: 'section.create', id: 'sx', name: 'One too many' })).toThrow(
      LocalizedError,
    );

    let full = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    for (let i = 0; i < limits.refsPerSection; i += 1) {
      full = applySidebarAction(full, { type: 'room.move', ref: `room-${i}`, sectionId: 's1' });
    }
    expect(full.sections[0]?.refs).toHaveLength(limits.refsPerSection);
    expect(() => applySidebarAction(full, { type: 'room.move', ref: 'room-over', sectionId: 's1' })).toThrow(LocalizedError);
  });

  it('stores a section name trimmed and in NFC form', () => {
    const doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: '  Café  ' });
    expect(doc.sections[0]?.name).toBe('Café');
  });

  it('refuses a blank name, and a name longer than the shared limit counted in characters', () => {
    expect(() => applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: '   ' })).toThrow(
      LocalizedError,
    );
    const fits = '\u{1F600}'.repeat(limits.name);
    const tooLong = '\u{1F600}'.repeat(limits.name + 1);
    expect(applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: fits }).sections[0]?.name).toBe(
      fits,
    );
    expect(() => applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: tooLong })).toThrow(
      LocalizedError,
    );
  });

  it('refuses a section name already taken, ignoring case, and a rename onto a taken name', () => {
    const doc = applySidebarAction(
      applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' }),
      { type: 'section.create', id: 's2', name: 'Home' },
    );
    expect(() => applySidebarAction(doc, { type: 'section.create', id: 's3', name: 'work' })).toThrow(
      'A section with this name already exists.',
    );
    expect(() => applySidebarAction(doc, { type: 'section.rename', id: 's2', name: 'WORK' })).toThrow(
      'A section with this name already exists.',
    );
    // Renaming a section to the name it already has is not a clash with itself.
    expect(applySidebarAction(doc, { type: 'section.rename', id: 's1', name: 'Work' }).sections[0]?.name).toBe('Work');
  });

  it('refuses a section id that already exists, and an action on a section that is gone', () => {
    const doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    expect(() => applySidebarAction(doc, { type: 'section.create', id: 's1', name: 'Other' })).toThrow(
      'Section already exists.',
    );
    expect(() => applySidebarAction(doc, { type: 'section.remove', id: 'missing' })).toThrow(
      'Section not found. It may have been removed.',
    );
    expect(() => applySidebarAction(doc, { type: 'section.collapse', id: 'missing', collapsed: true })).toThrow(LocalizedError);
  });

  it('renames, collapses, sorts and reorders sections', () => {
    let doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    doc = applySidebarAction(doc, { type: 'section.create', id: 's2', name: 'Home' });
    doc = applySidebarAction(doc, { type: 'section.rename', id: 's1', name: 'Projects' });
    doc = applySidebarAction(doc, { type: 'section.collapse', id: 's2', collapsed: true });
    doc = applySidebarAction(doc, { type: 'section.sort', id: 's2', sort: 'name' });
    doc = applySidebarAction(doc, { type: 'section.move', id: 's2', beforeId: 's1' });
    expect(doc.sections.map((entry) => [entry.id, entry.name, entry.collapsed, entry.sort])).toEqual([
      ['s2', 'Home', true, 'name'],
      ['s1', 'Projects', false, 'manual'],
    ]);
  });

  it('keeps each room in at most one section, and moves it out of Starred and Hidden', () => {
    let doc = applySidebarAction(emptySidebarDoc(), { type: 'star', ref: 'room-a' });
    doc = applySidebarAction(doc, { type: 'section.create', id: 's1', name: 'Work' });
    doc = applySidebarAction(doc, { type: 'section.create', id: 's2', name: 'Home' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: 's1' });
    expect(doc.starred).toEqual([]);
    doc = applySidebarAction(doc, { type: 'room.hide', ref: 'room-b' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-b', sectionId: 's2' });
    expect(doc.hidden).toEqual([]);
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: 's2' });
    expect(doc.sections.map((entry) => entry.refs)).toEqual([[], ['room-b', 'room-a']]);
  });

  it('places a room before another room in its section, which makes that section manual', () => {
    let doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-b', sectionId: 's1' });
    doc = applySidebarAction(doc, { type: 'section.sort', id: 's1', sort: 'name' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-c', sectionId: 's1', beforeRef: 'room-b' });
    expect(doc.sections[0]?.refs).toEqual(['room-c', 'room-b']);
    expect(doc.sections[0]?.sort).toBe('manual');
  });

  it('removes a room from every section when it is moved to no section, and refuses a section that is gone', () => {
    let doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: 's1' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: null });
    expect(doc.sections[0]?.refs).toEqual([]);
    expect(() => applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: 'nope' })).toThrow(LocalizedError);
  });

  it('removes a section, and with it the membership its rooms had', () => {
    let doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    doc = applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: 's1' });
    doc = applySidebarAction(doc, { type: 'section.remove', id: 's1' });
    expect(doc.sections).toEqual([]);
    expect(refsOf(modelOf([room('room-a')], doc).groups[0]?.rooms ?? [])).toEqual(['room-a']);
  });

  it('updates a built-in group, and drops its entry when nothing about it is left to keep', () => {
    let doc = applySidebarAction(emptySidebarDoc(), { type: 'group.update', groupId: 'dms', sort: 'name' });
    expect(doc.groups).toEqual([{ id: 'dms', sort: 'name' }]);
    doc = applySidebarAction(doc, { type: 'group.update', groupId: 'dms', sort: null });
    expect(doc.groups).toEqual([]);
    doc = applySidebarAction(doc, { type: 'group.update', groupId: 'channels', collapsed: true });
    expect(doc.groups).toEqual([{ id: 'channels', collapsed: true }]);
  });

  it('sets a computed slot on or off', () => {
    const on = applySidebarAction(emptySidebarDoc(), { type: 'computed.toggle', key: 'unread', on: true });
    expect(on.computed.unread).toBe(true);
    expect(applySidebarAction(on, { type: 'computed.toggle', key: 'unread', on: false }).computed.unread).toBe(false);
  });

  it('refuses an action that does not match the wire schema, and never mutates the document it was given', () => {
    expect(() => applySidebarAction(emptySidebarDoc(), { type: 'star', ref: 'not a valid ref' } as never)).toThrow();
    expect(() => applySidebarAction(emptySidebarDoc(), { type: 'teleport' } as never)).toThrow();

    const doc = applySidebarAction(emptySidebarDoc(), { type: 'section.create', id: 's1', name: 'Work' });
    const before = JSON.stringify(doc);
    applySidebarAction(doc, { type: 'room.move', ref: 'room-a', sectionId: 's1' });
    applySidebarAction(doc, { type: 'star', ref: 'room-a' });
    expect(JSON.stringify(doc)).toBe(before);
  });
});

describe('pruneDoc (WS16-S9)', () => {
  it('drops every reference to a room the viewer cannot see', () => {
    const doc = docOf({
      starred: ['room-a', 'gone'],
      hidden: ['gone-2'],
      sections: [section('s1', 'Work', ['room-a', 'gone'])],
    });
    const pruned = pruneDoc(doc, new Set(['room-a']));
    expect(pruned.starred).toEqual(['room-a']);
    expect(pruned.hidden).toEqual([]);
    expect(pruned.sections[0]?.refs).toEqual(['room-a']);
    expect(doc.starred).toEqual(['room-a', 'gone']);
  });

  it('returns the same document when nothing needs pruning', () => {
    const doc = docOf({ starred: ['room-a'], sections: [section('s1', 'Work', ['room-a'])] });
    expect(pruneDoc(doc, new Set(['room-a']))).toBe(doc);
  });
});

describe('buildSidebarModel slot table (WS16-S9)', () => {
  it('lists Starred, then sections, then groups, and never shows a room twice', () => {
    const spaces = [room('room-a'), room('room-b'), room('room-c', 'room-c', 'dm'), room('room-d'), room('room-e')];
    const doc = docOf({
      starred: ['room-d', 'room-a'],
      sections: [section('s1', 'Work', ['room-b', 'room-d']), section('s2', 'Empty', [])],
    });
    const model = modelOf(spaces, doc);
    expect(refsOf(model.starred)).toEqual(['room-d', 'room-a']);
    expect(model.sections.map((entry) => [entry.section.id, refsOf(entry.rooms)])).toEqual([
      ['s1', ['room-b']],
      ['s2', []],
    ]);
    expect(model.groups.map((group) => [group.id, refsOf(group.rooms)])).toEqual([
      ['channels', ['room-e']],
      ['dms', ['room-c']],
    ]);
    expect(model.starred.length + model.sections.flatMap((entry) => entry.rooms).length + model.groups.flatMap((group) => group.rooms).length).toBe(spaces.length);
  });

  it('sorts a section by name only when the section asks for it', () => {
    const spaces = [room('room-b', 'Beta'), room('room-a', 'Alpha')];
    const manual = modelOf(spaces, docOf({ sections: [section('s1', 'Work', ['room-b', 'room-a'])] }));
    expect(sectionRefs(manual, 's1')).toEqual(['room-b', 'room-a']);
    const byName = modelOf(spaces, docOf({ sections: [section('s1', 'Work', ['room-b', 'room-a'], 'name')] }));
    expect(sectionRefs(byName, 's1')).toEqual(['room-a', 'room-b']);
  });

  it('applies the sort and collapsed state of a built-in group, and omits a group with no rooms', () => {
    const spaces = [room('room-b', 'Beta'), room('room-a', 'Alpha')];
    const model = modelOf(spaces, docOf({ groups: [{ id: 'channels', sort: 'name', collapsed: true }] }));
    expect(model.groups.map((group) => group.id)).toEqual(['channels']);
    expect(model.groups[0]?.collapsed).toBe(true);
    expect(groupRefs(model, 'channels')).toEqual(['room-a', 'room-b']);
    expect(modelOf([room('room-a')], emptySidebarDoc()).groups.map((group) => group.id)).toEqual(['channels']);
  });

  it('places a kind no registered group claims into Channels, so it is never dropped', () => {
    const spaces = [room('room-a'), room('room-z', 'Zeta', 'future_kind'), room('room-d', 'Dee', 'dm')];
    const model = modelOf(spaces, emptySidebarDoc());
    expect(model.groups.map((group) => [group.id, refsOf(group.rooms)])).toEqual([
      ['channels', ['room-a', 'room-z']],
      ['dms', ['room-d']],
    ]);
  });

  it('keeps hidden rooms out of Starred, sections and groups, and lists them in the Hidden slot', () => {
    const spaces = [room('room-a'), room('room-h', 'Hidden room')];
    const doc = docOf({
      starred: ['room-h'],
      hidden: ['room-h'],
      sections: [section('s1', 'Work', ['room-h'])],
    });
    const model = modelOf(spaces, doc);
    expect(refsOf(model.starred)).toEqual([]);
    expect(sectionRefs(model, 's1')).toEqual([]);
    expect(groupRefs(model, 'channels')).toEqual(['room-a']);
    // The model keeps the Hidden slot whole; the panel shows it only behind "Show hidden" or a search.
    expect(refsOf(model.hidden)).toEqual(['room-h']);
    expect(model.hiddenCount).toBe(1);
  });

  it('narrows every slot to the search, and brings a hidden room back only when it matches', () => {
    const spaces = [room('room-a', 'Budget'), room('room-b', 'Garden'), room('room-h', 'Budget archive')];
    const doc = docOf({ hidden: ['room-h'] });
    const searching = modelOf(spaces, doc, { query: 'budget' });
    expect(refsOf(searching.hidden)).toEqual(['room-h']);
    expect(groupRefs(searching, 'channels')).toEqual(['room-a']);
    const other = modelOf(spaces, doc, { query: 'garden' });
    expect(other.hidden).toEqual([]);
    expect(groupRefs(other, 'channels')).toEqual(['room-b']);
    expect(other.hiddenCount).toBe(1);
  });

  it('renders no overlays and no muted list without an attention answer', () => {
    const doc = docOf({ computed: { needsYou: true, unread: true, working: true, muted: true }, starred: ['room-m'] });
    const model = modelOf([room('room-a'), room('room-m')], doc);
    expect(model.needsYou).toEqual([]);
    expect(model.unread).toEqual([]);
    expect(model.working).toEqual([]);
    expect(model.muted).toEqual([]);
    expect(model.mutedRefs.size).toBe(0);
    expect(refsOf(model.starred)).toEqual(['room-m']);
  });
});

const quiet: SidebarRoomAttention = { needsYou: 0, mentions: 0, unreadThreads: 0, working: 0, level: 'all' };
const answers = (table: Record<string, Partial<SidebarRoomAttention>>) => {
  const byRef = new Map(Object.entries(table).map(([ref, patch]) => [ref, { ...quiet, ...patch }]));
  return (ref: string) => byRef.get(ref);
};
const ALL_ON = { needsYou: true, unread: true, working: true, muted: true };

describe('buildSidebarModel attention slots (WS16-S10)', () => {
  it('shows a room that needs you in Needs you even when it is hidden or muted, and keeps its home slot', () => {
    const spaces = [room('room-a'), room('room-h'), room('room-m')];
    const doc = docOf({
      computed: ALL_ON,
      starred: ['room-a'],
      hidden: ['room-h'],
      sections: [section('s1', 'Work', ['room-m'])],
    });
    const attention = answers({
      'room-a': { needsYou: 1 },
      'room-h': { mentions: 2 },
      'room-m': { needsYou: 1, level: 'off' },
    });
    const model = modelOf(spaces, doc, { attention });
    expect(refsOf(model.needsYou)).toEqual(['room-a', 'room-h', 'room-m']);
    expect(refsOf(model.starred)).toEqual(['room-a']);
    expect(refsOf(model.hidden)).toEqual(['room-h']);
  });

  it('shows unread rooms in Unread, but not muted rooms or hidden ones', () => {
    const spaces = [room('room-u'), room('room-off'), room('room-h')];
    const doc = docOf({ computed: ALL_ON, hidden: ['room-h'] });
    const attention = answers({
      'room-u': { unreadThreads: 2 },
      'room-off': { unreadThreads: 3, level: 'off' },
      'room-h': { unreadThreads: 1 },
    });
    expect(refsOf(modelOf(spaces, doc, { attention }).unread)).toEqual(['room-u']);
  });

  it('shows rooms with agent work running in Agent work, only while that slot is on', () => {
    const spaces = [room('room-w'), room('room-q')];
    const attention = answers({ 'room-w': { working: 1 }, 'room-q': { working: 0 } });
    expect(refsOf(modelOf(spaces, docOf({ computed: ALL_ON }), { attention }).working)).toEqual(['room-w']);
    expect(refsOf(modelOf(spaces, docOf({ computed: { ...ALL_ON, working: false } }), { attention }).working)).toEqual([]);
  });

  it('places a room in at most one overlay: Needs you, then Unread, then Agent work', () => {
    const spaces = [room('room-x'), room('room-y')];
    const attention = answers({
      'room-x': { needsYou: 1, unreadThreads: 2, working: 1 },
      'room-y': { unreadThreads: 1, working: 1 },
    });
    const model = modelOf(spaces, docOf({ computed: ALL_ON }), { attention });
    expect(refsOf(model.needsYou)).toEqual(['room-x']);
    expect(refsOf(model.unread)).toEqual(['room-y']);
    expect(refsOf(model.working)).toEqual([]);
    // With Needs you switched off, the same room falls through to Unread.
    const noNeeds = modelOf(spaces, docOf({ computed: { ...ALL_ON, needsYou: false } }), { attention });
    expect(refsOf(noNeeds.needsYou)).toEqual([]);
    expect(refsOf(noNeeds.unread)).toEqual(['room-x', 'room-y']);
  });

  it('keeps every muted room in mutedRefs whether or not Muted is on, and moves it only when Muted is on', () => {
    const spaces = [room('room-m', 'Muted starred'), room('room-s'), room('room-g'), room('room-mh')];
    const attention = answers({
      'room-m': { level: 'off' },
      'room-s': { level: 'off' },
      'room-g': { level: 'off' },
      'room-mh': { level: 'off' },
    });
    const base = docOf({
      starred: ['room-m'],
      hidden: ['room-mh'],
      sections: [section('s1', 'Work', ['room-s'])],
    });

    const off = modelOf(spaces, base, { attention });
    expect([...off.mutedRefs].sort()).toEqual(['room-g', 'room-m', 'room-mh', 'room-s']);
    expect(off.muted).toEqual([]);
    expect(refsOf(off.starred)).toEqual(['room-m']);
    expect(sectionRefs(off, 's1')).toEqual(['room-s']);
    expect(groupRefs(off, 'channels')).toEqual(['room-g']);

    const on = modelOf(spaces, docOf({ ...base, computed: { ...base.computed, muted: true } }), { attention });
    expect(refsOf(on.muted)).toEqual(['room-m', 'room-s', 'room-g']);
    expect(refsOf(on.starred)).toEqual([]);
    expect(sectionRefs(on, 's1')).toEqual([]);
    expect(on.groups).toEqual([]);
    // A muted room that is also hidden stays in Hidden, not in Muted.
    expect(refsOf(on.hidden)).toEqual(['room-mh']);
  });
});
