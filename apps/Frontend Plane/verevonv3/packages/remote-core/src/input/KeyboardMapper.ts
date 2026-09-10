import { ProtocolError } from '../errors/RemoteError.js';
import { ControlKey, type RdKeyIdentification } from '../protocol/rustdesk/messages/types.js';

/**
 * Nettleserens `KeyboardEvent.key`-navn → RustDesk sin `ControlKey`-enum.
 * Kun ikke-skrivbare taster hører hjemme her; skrivbare tegn går som unicode.
 */
const CONTROL_KEY_BY_BROWSER_KEY: Readonly<Record<string, number>> = {
  Enter: ControlKey.Return,
  Backspace: ControlKey.Backspace,
  Tab: ControlKey.Tab,
  Escape: ControlKey.Escape,
  ' ': ControlKey.Space,
  ArrowUp: ControlKey.UpArrow,
  ArrowDown: ControlKey.DownArrow,
  ArrowLeft: ControlKey.LeftArrow,
  ArrowRight: ControlKey.RightArrow,
  Delete: ControlKey.Delete,
  Home: ControlKey.Home,
  End: ControlKey.End,
  PageUp: ControlKey.PageUp,
  PageDown: ControlKey.PageDown,
  Insert: ControlKey.Insert,
  Control: ControlKey.Control,
  Shift: ControlKey.Shift,
  Alt: ControlKey.Alt,
  Meta: ControlKey.Meta,
  CapsLock: ControlKey.CapsLock,
  NumLock: ControlKey.NumLock,
  ScrollLock: ControlKey.Scroll,
  Pause: ControlKey.Pause,
  ContextMenu: ControlKey.Apps,
  F1: ControlKey.F1,
  F2: ControlKey.F2,
  F3: ControlKey.F3,
  F4: ControlKey.F4,
  F5: ControlKey.F5,
  F6: ControlKey.F6,
  F7: ControlKey.F7,
  F8: ControlKey.F8,
  F9: ControlKey.F9,
  F10: ControlKey.F10,
  F11: ControlKey.F11,
  F12: ControlKey.F12,
  AudioVolumeMute: ControlKey.VolumeMute,
  AudioVolumeUp: ControlKey.VolumeUp,
  AudioVolumeDown: ControlKey.VolumeDown,
};

/**
 * Oversetter et tastenavn til RustDesk sin `KeyEvent`-identifikasjon.
 *
 * VIKTIG BEGRENSNING: RustDesk sin "Map"-modus sender POSISJONSBASERTE
 * scancodes (nettleserens `KeyboardEvent.code` → USB HID → Windows scancode).
 * Den oversettelsestabellen finnes bare inne i RustDesk sin egen `rdev`-krate,
 * ikke som en publisert spesifikasjon — se docs/rustdesk-protocol.md,
 * "Browser Translation Concerns". Vi bruker derfor navngitte ControlKey-er og
 * unicode-kodepunkter, som fungerer for vanlig tekst- og navigasjonsbruk, men
 * IKKE for programvare som leser scancodes direkte (spill, noen
 * remote-desktop-i-remote-desktop-oppsett). Det er en kjent, dokumentert
 * begrensning — ikke en stille feil.
 */
export function mapBrowserKeyToIdentification(key: string): RdKeyIdentification {
  const controlKey = CONTROL_KEY_BY_BROWSER_KEY[key];
  if (controlKey !== undefined) {
    return { kind: 'controlKey', controlKey };
  }

  // `[...key]` teller kodepunkter, ikke UTF-16-enheter, så emoji og andre
  // tegn utenfor BMP regnes riktig som ett tegn.
  const codePoints = [...key];
  if (codePoints.length === 1) {
    const codepoint = key.codePointAt(0);
    if (codepoint !== undefined) {
      return { kind: 'unicode', codepoint };
    }
  }

  throw new ProtocolError(`Unsupported key name for the remote keyboard: "${key}"`, { key });
}

/** Sammensatt tekst (IME, innliming, `keyboard.type`) sendes som én sekvens. */
export function mapTextToIdentification(text: string): RdKeyIdentification {
  return { kind: 'seq', text };
}
