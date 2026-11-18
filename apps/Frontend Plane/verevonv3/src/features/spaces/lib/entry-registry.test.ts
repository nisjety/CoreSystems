import { describe, expect, it, vi } from 'vitest';

import { turnEntryFromWire, type AgentCardEntry, type TurnEntry, type TurnEntryCard } from './entry-registry';

// WS15-S9, WS18-S7 and W3-FIX-16: typed room entries and the card registry. `turnEntryFromWire` reads the
// wire form of one message's entry; the registry says which card draws each kind. The registry keeps module
// state, so the registry cases load a fresh copy of the module each time.

const card: TurnEntryCard = () => null;
const otherCard: TurnEntryCard = () => null;
const fallback: TurnEntryCard = () => null;

async function freshRegistry() {
  vi.resetModules();
  return import('./entry-registry');
}

const fieldsOf = (entry: TurnEntry | undefined) => (entry?.kind === 'agent_card' ? entry.fields : undefined);

const agentEntry: TurnEntry = { kind: 'agent_card', version: 1, title: '', tone: undefined, fields: [] };

describe('turnEntryFromWire, agent cards (W3-FIX-16)', () => {
  it('fills an agent card from the gateway wire shape: kind, version, and agent_card with title, tone and fields', () => {
    expect(
      turnEntryFromWire({
        kind: 'agent_card',
        version: 1,
        agent_card: { title: 'Build', tone: 'progress', fields: [{ label: 'Step', value: '2 of 4' }] },
      }),
    ).toEqual({
      kind: 'agent_card',
      version: 1,
      title: 'Build',
      tone: 'progress',
      fields: [{ label: 'Step', value: '2 of 4' }],
    });
  });
});

describe('turnEntryFromWire (WS15-S9)', () => {
  it('returns nothing for a message that carries no entry', () => {
    expect(turnEntryFromWire(undefined)).toBeUndefined();
    expect(turnEntryFromWire(null)).toBeUndefined();
    expect(turnEntryFromWire('text')).toBeUndefined();
    expect(turnEntryFromWire({})).toBeUndefined();
    expect(turnEntryFromWire({ kind: '   ', version: 1 })).toBeUndefined();
  });

  it('keeps the kind and version of an unknown kind, and cuts the kind at 40 characters', () => {
    expect(turnEntryFromWire({ kind: 'poll', version: 1 })).toEqual({ kind: 'unknown', rawKind: 'poll', version: 1 });
    expect(turnEntryFromWire({ kind: 'k'.repeat(60), version: 1 })).toEqual({
      kind: 'unknown',
      rawKind: 'k'.repeat(40),
      version: 1,
    });
  });

  it('treats a newer version of a known kind as unknown, not as the kind it names', () => {
    const v2 = {
      kind: 'link_previews',
      version: 2,
      link_previews: [{ url: 'https://news.example.test/a', title: 'Story' }],
    };
    expect(turnEntryFromWire(v2)).toEqual({ kind: 'unknown', rawKind: 'link_previews', version: 2 });
  });

  it('turns a payload this build cannot read into the unknown entry, and never throws', () => {
    expect(turnEntryFromWire({ kind: 'link_previews', version: 1, link_previews: 'nope' })).toEqual({
      kind: 'unknown',
      rawKind: 'link_previews',
      version: 1,
    });
    expect(turnEntryFromWire({ kind: 'link_previews', version: 1, link_previews: [null, 42, { url: 'https://' }] })).toEqual({
      kind: 'unknown',
      rawKind: 'link_previews',
      version: 1,
    });
  });

  it('drops an unsafe javascript: link and keeps the safe one beside it', () => {
    const entry = turnEntryFromWire({
      kind: 'link_previews',
      version: 1,
      link_previews: [
        { url: 'javascript:alert(1)', title: 'Bad' },
        { url: 'https://news.example.test/a', title: 'Good' },
      ],
    });
    expect(entry).toEqual({
      kind: 'link_previews',
      version: 1,
      items: [
        {
          href: 'https://news.example.test/a',
          host: 'news.example.test',
          title: 'Good',
          description: '',
          siteName: '',
          fetchedAt: '',
        },
      ],
    });
    expect(
      turnEntryFromWire({ kind: 'link_previews', version: 1, link_previews: [{ url: 'javascript:alert(1)' }] }),
    ).toEqual({ kind: 'unknown', rawKind: 'link_previews', version: 1 });
  });

  it('reads the host from the link itself, never from a host field the server sent', () => {
    const entry = turnEntryFromWire({
      kind: 'link_previews',
      version: 1,
      link_previews: [{ url: 'https://Docs.Example.Test/guide', host: 'phish.example.net', title: 'Guide' }],
    });
    expect(entry).toMatchObject({ kind: 'link_previews', items: [{ href: 'https://Docs.Example.Test/guide', host: 'docs.example.test' }] });
  });

  it('opens the final URL and reads the host from it, and keeps only a parseable fetch time', () => {
    const entry = turnEntryFromWire({
      kind: 'link_previews',
      version: 1,
      link_previews: [
        {
          url: 'https://short.example.test/x',
          final_url: 'https://news.example.test/a',
          title: 'Story',
          fetched_at: '2026-11-01T10:00:00Z',
        },
        { url: 'https://other.example.test/b', fetched_at: 'not a date' },
      ],
    });
    expect(entry).toMatchObject({
      kind: 'link_previews',
      items: [
        { href: 'https://news.example.test/a', host: 'news.example.test', fetchedAt: '2026-11-01T10:00:00.000Z' },
        { href: 'https://other.example.test/b', host: 'other.example.test', fetchedAt: '' },
      ],
    });
  });
});

describe('entry card registry (WS15-S9)', () => {
  it('draws a kind with the card registered for it', async () => {
    const { registerTurnEntryCard, turnEntryCard } = await freshRegistry();
    registerTurnEntryCard('link_previews', card);
    expect(turnEntryCard({ kind: 'link_previews', version: 1, items: [] })).toBe(card);
  });

  it('draws a kind with no card of its own with the neutral fallback', async () => {
    const { registerFallbackTurnEntryCard, registerTurnEntryCard, turnEntryCard } = await freshRegistry();
    registerFallbackTurnEntryCard(fallback);
    registerTurnEntryCard('link_previews', card);
    expect(turnEntryCard({ kind: 'unknown', rawKind: 'poll', version: 1 })).toBe(fallback);
    expect(turnEntryCard(agentEntry)).toBe(fallback);
  });

  it('has no card to draw with when neither a card nor a fallback is registered', async () => {
    const { turnEntryCard } = await freshRegistry();
    expect(turnEntryCard({ kind: 'unknown', rawKind: 'poll', version: 1 })).toBeUndefined();
  });

  it('a later registration of a kind replaces the earlier card, so the last module to load wins', async () => {
    const { registerTurnEntryCard, turnEntryCard } = await freshRegistry();
    registerTurnEntryCard('agent_card', card);
    registerTurnEntryCard('agent_card', otherCard);
    expect(turnEntryCard(agentEntry)).toBe(otherCard);
  });
});

describe('agent cards (WS18-S7)', () => {
  it('keeps every agent_card an agent card, whatever its version or payload', () => {
    expect(
      turnEntryFromWire({ kind: 'agent_card', version: 2, agent_card: { title: 'New', tone: 'done', fields: [] } }),
    ).toEqual({ kind: 'agent_card', version: 2, title: '', tone: undefined, fields: [] });
    expect(turnEntryFromWire({ kind: 'agent_card', version: 1 })).toEqual({
      kind: 'agent_card',
      version: 1,
      title: '',
      tone: undefined,
      fields: [],
    });
    expect(turnEntryFromWire({ kind: 'agent_card', agent_card: { title: 'No version' } })).toEqual({
      kind: 'agent_card',
      version: 0,
      title: '',
      tone: undefined,
      fields: [],
    });
  });

  it('fills title, tone and fields from a version 1 payload, and strips control characters', () => {
    const entry = turnEntryFromWire({
      kind: 'agent_card',
      version: 1,
      agent_card: {
        title: 'Build\u0007 done\n',
        tone: 'blocked',
        fields: [{ label: 'Sta\u0000ge', value: 'Waiting​' }],
      },
    });
    expect(entry).toEqual({
      kind: 'agent_card',
      version: 1,
      title: 'Build done',
      tone: 'blocked',
      fields: [{ label: 'Stage', value: 'Waiting' }],
    });
  });

  it('drops a tone the card does not know and keeps one it does', () => {
    const unknown = turnEntryFromWire({ kind: 'agent_card', version: 1, agent_card: { title: 'x', tone: 'urgent' } });
    expect(unknown).toEqual({ kind: 'agent_card', version: 1, title: 'x', tone: undefined, fields: [] });
    const known = turnEntryFromWire({ kind: 'agent_card', version: 1, agent_card: { title: 'x', tone: 'info' } });
    expect((known as AgentCardEntry).tone).toBe('info');
  });

  it('caps the fields at eight, and drops a field with no label or no value', () => {
    const many = Array.from({ length: 10 }, (_, index) => ({ label: `L${index}`, value: `V${index}` }));
    const capped = turnEntryFromWire({ kind: 'agent_card', version: 1, agent_card: { title: 'Many', fields: many } });
    expect(fieldsOf(capped)).toEqual(many.slice(0, 8));

    const partial = turnEntryFromWire({
      kind: 'agent_card',
      version: 1,
      agent_card: {
        title: 'Partial',
        fields: [
          { label: '', value: 'x' },
          { label: 'Ok', value: '   ' },
          { label: 'Kept', value: 'yes' },
        ],
      },
    });
    expect(fieldsOf(partial)).toEqual([{ label: 'Kept', value: 'yes' }]);
  });

  it('caps the title at 120 characters', () => {
    const entry = turnEntryFromWire({ kind: 'agent_card', version: 1, agent_card: { title: 'x'.repeat(150) } });
    expect(entry).toMatchObject({ kind: 'agent_card', title: 'x'.repeat(120) });
  });

  it('a card registered with replacesBody makes the timeline draw the text itself, for that kind only', async () => {
    const { registerTurnEntryCard, turnEntryReplacesBody } = await freshRegistry();
    registerTurnEntryCard('agent_card', card, { replacesBody: true });
    expect(turnEntryReplacesBody(agentEntry)).toBe(true);
    expect(turnEntryReplacesBody({ kind: 'unknown', rawKind: 'poll', version: 1 })).toBe(false);
    expect(turnEntryReplacesBody({ kind: 'link_previews', version: 1, items: [] })).toBe(false);
  });

  it('re-registering a kind without replacesBody clears an earlier replacesBody for it', async () => {
    const { registerTurnEntryCard, turnEntryReplacesBody } = await freshRegistry();
    registerTurnEntryCard('agent_card', card, { replacesBody: true });
    expect(turnEntryReplacesBody(agentEntry)).toBe(true);
    registerTurnEntryCard('agent_card', otherCard);
    expect(turnEntryReplacesBody(agentEntry)).toBe(false);
  });
});
