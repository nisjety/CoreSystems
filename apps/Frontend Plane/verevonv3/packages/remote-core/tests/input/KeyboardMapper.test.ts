import { describe, expect, it } from 'vitest';
import { mapBrowserKeyToIdentification, mapTextToIdentification } from '../../src/input/KeyboardMapper.js';
import { ControlKey } from '../../src/protocol/rustdesk/messages/types.js';
import { ProtocolError } from '../../src/errors/RemoteError.js';

describe('KeyboardMapper', () => {
  it('maps named non-printable keys to ControlKey values', () => {
    expect(mapBrowserKeyToIdentification('Enter')).toEqual({ kind: 'controlKey', controlKey: ControlKey.Return });
    expect(mapBrowserKeyToIdentification('Backspace')).toEqual({
      kind: 'controlKey',
      controlKey: ControlKey.Backspace,
    });
    expect(mapBrowserKeyToIdentification('ArrowLeft')).toEqual({
      kind: 'controlKey',
      controlKey: ControlKey.LeftArrow,
    });
    expect(mapBrowserKeyToIdentification('Control')).toEqual({
      kind: 'controlKey',
      controlKey: ControlKey.Control,
    });
  });

  it('maps the F-keys correctly despite their non-sequential enum values', () => {
    // RustDesk's ControlKey enum orders these F1=9, F10=10, F11=11, F12=12,
    // F2=13 … — a mapping bug here would be silent and confusing.
    expect(mapBrowserKeyToIdentification('F1')).toEqual({ kind: 'controlKey', controlKey: 9 });
    expect(mapBrowserKeyToIdentification('F2')).toEqual({ kind: 'controlKey', controlKey: 13 });
    expect(mapBrowserKeyToIdentification('F10')).toEqual({ kind: 'controlKey', controlKey: 10 });
    expect(mapBrowserKeyToIdentification('F12')).toEqual({ kind: 'controlKey', controlKey: 12 });
  });

  it('maps space to ControlKey.Space rather than a unicode codepoint', () => {
    expect(mapBrowserKeyToIdentification(' ')).toEqual({ kind: 'controlKey', controlKey: ControlKey.Space });
  });

  it('maps printable characters to their unicode codepoint', () => {
    expect(mapBrowserKeyToIdentification('a')).toEqual({ kind: 'unicode', codepoint: 97 });
    expect(mapBrowserKeyToIdentification('Ø')).toEqual({ kind: 'unicode', codepoint: 'Ø'.codePointAt(0) });
  });

  it('treats an astral-plane character as one key, not two UTF-16 units', () => {
    const emoji = '😀';
    expect(mapBrowserKeyToIdentification(emoji)).toEqual({
      kind: 'unicode',
      codepoint: emoji.codePointAt(0),
    });
  });

  it('throws on an unmappable key name instead of silently sending nothing', () => {
    expect(() => mapBrowserKeyToIdentification('MediaTrackNext')).toThrow(ProtocolError);
    expect(() => mapBrowserKeyToIdentification('')).toThrow(ProtocolError);
  });

  it('maps composed text to a sequence', () => {
    expect(mapTextToIdentification('hei på deg')).toEqual({ kind: 'seq', text: 'hei på deg' });
  });
});
