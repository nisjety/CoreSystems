/**
 * Meldingsformer for RustDesk sin wire-protokoll, uttrykt som egne TypeScript-
 * typer — IKKE oversatt fra eller kopiert av .proto-filene. Feltnavn/-numre
 * er verifisert direkte mot rustdesk/hbb_common (se docs/rustdesk-protocol.md
 * for kilder/commit-SHA-er) og reimplementert her som protokollfakta, slik
 * spec-en krever ("prefer clean protocol implementation").
 *
 * Fire navn er prefikset med `Rd` (RustDesk) fordi de ellers ville skygget
 * for globale DOM-/WebCodecs-typer i samme modul (`MouseEvent`, `KeyEvent`,
 * `Clipboard`, `VideoFrame`) — resten beholder RustDesk sine egne navn siden
 * de ikke kolliderer med noe.
 */

// ---------- Håndtrykk / innlogging ----------

export interface RdHash {
  readonly salt: string;
  readonly challenge: string;
}

export interface RdPublicKey {
  readonly asymmetricValue: Uint8Array;
  readonly symmetricValue: Uint8Array;
}

export interface RdSignedId {
  readonly id: Uint8Array;
}

export interface RdIdPk {
  readonly id: string;
  readonly pk: Uint8Array;
}

export interface RdOSLogin {
  readonly username: string;
  readonly password: string;
}

export interface RdLoginRequest {
  /** Målets peer-ID — ja, feltnavnet i protokollen er "username", men det er faktisk peer-ID-en man kobler til. */
  readonly username: string;
  readonly password: Uint8Array;
  readonly myId: string;
  readonly myName: string;
  readonly myPlatform: string;
  readonly sessionId: bigint;
  readonly version: string;
  readonly videoAckRequired: boolean;
  readonly hwid: Uint8Array;
  readonly avatar: string;
  readonly osLogin?: RdOSLogin;
  readonly option?: RdOptionMessage;
}

export type RdLoginResult =
  | { readonly kind: 'error'; readonly error: string }
  | { readonly kind: 'peerInfo'; readonly peerInfo: RdPeerInfo };

export interface RdLoginResponse {
  readonly result: RdLoginResult;
  readonly enableTrustedDevices: boolean;
}

// ---------- Skjerm / oppløsning ----------

export interface RdResolution {
  readonly width: number;
  readonly height: number;
}

export interface RdDisplayInfo {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly name: string;
  readonly online: boolean;
  readonly cursorEmbedded: boolean;
  readonly originalResolution?: RdResolution;
  readonly scale: number;
}

export interface RdPeerInfo {
  readonly username: string;
  readonly hostname: string;
  readonly platform: string;
  readonly displays: readonly RdDisplayInfo[];
  readonly currentDisplay: number;
  readonly sasEnabled: boolean;
  readonly version: string;
}

// ---------- Pekerinput ----------

/** Lav 3 bit av MouseEvent.mask. Verdiene er RustDesk sine, ikke oppfunnet her. */
export const MouseEventKind = {
  Move: 0,
  Down: 1,
  Up: 2,
  Wheel: 3,
  Trackpad: 4,
  MoveRelative: 5,
} as const;

/** Høye biter av MouseEvent.mask (skiftet 3 til venstre før OR med kind). */
export const MouseButtonFlag = {
  Left: 0x01,
  Right: 0x02,
  Wheel: 0x04,
  Back: 0x08,
  Forward: 0x10,
} as const;

export interface RdMouseEvent {
  readonly kind: number;
  readonly buttonFlags: number;
  readonly x: number;
  readonly y: number;
  readonly modifiers: readonly number[];
}

// ---------- Tastaturinput ----------

export const KeyboardMode = { Legacy: 0, Map: 1, Translate: 2, Auto: 3 } as const;

export type RdKeyIdentification =
  | { readonly kind: 'controlKey'; readonly controlKey: number }
  | { readonly kind: 'chr'; readonly code: number }
  | { readonly kind: 'unicode'; readonly codepoint: number }
  | { readonly kind: 'seq'; readonly text: string };

export interface RdKeyEvent {
  readonly down: boolean;
  readonly press: boolean;
  readonly identification: RdKeyIdentification;
  readonly modifiers: readonly number[];
  readonly mode: number;
}

/** Fullstendig ControlKey-tabell fra hbb_common/protos/message.proto. */
export const ControlKey = {
  Unknown: 0,
  Alt: 1,
  Backspace: 2,
  CapsLock: 3,
  Control: 4,
  Delete: 5,
  DownArrow: 6,
  End: 7,
  Escape: 8,
  F1: 9,
  F10: 10,
  F11: 11,
  F12: 12,
  F2: 13,
  F3: 14,
  F4: 15,
  F5: 16,
  F6: 17,
  F7: 18,
  F8: 19,
  F9: 20,
  Home: 21,
  LeftArrow: 22,
  Meta: 23,
  Option: 24,
  PageDown: 25,
  PageUp: 26,
  Return: 27,
  RightArrow: 28,
  Shift: 29,
  Space: 30,
  Tab: 31,
  UpArrow: 32,
  Numpad0: 33,
  Numpad1: 34,
  Numpad2: 35,
  Numpad3: 36,
  Numpad4: 37,
  Numpad5: 38,
  Numpad6: 39,
  Numpad7: 40,
  Numpad8: 41,
  Numpad9: 42,
  Cancel: 43,
  Clear: 44,
  Menu: 45,
  Pause: 46,
  Insert: 58,
  Help: 59,
  Sleep: 60,
  Separator: 61,
  Scroll: 62,
  NumLock: 63,
  RWin: 64,
  Apps: 65,
  Multiply: 66,
  Add: 67,
  Subtract: 68,
  Decimal: 69,
  Divide: 70,
  Equals: 71,
  NumpadEnter: 72,
  RShift: 73,
  RControl: 74,
  RAlt: 75,
  VolumeMute: 76,
  VolumeUp: 77,
  VolumeDown: 78,
  Power: 79,
  CtrlAltDel: 100,
  LockScreen: 101,
} as const;

// ---------- Utklippstavle ----------

export const ClipboardFormat = {
  Text: 0,
  Rtf: 1,
  Html: 2,
  ImageRgba: 21,
  ImagePng: 22,
  ImageSvg: 23,
  Special: 31,
} as const;

export interface RdClipboard {
  readonly compress: boolean;
  readonly content: Uint8Array;
  readonly width: number;
  readonly height: number;
  readonly format: number;
  readonly specialName: string;
}

// ---------- Tillatelser ----------

export const Permission = {
  Keyboard: 0,
  Clipboard: 2,
  Audio: 3,
  File: 4,
  Restart: 5,
  Recording: 6,
  BlockInput: 7,
  PrivacyMode: 8,
} as const;

export interface RdPermissionInfo {
  readonly permission: number;
  readonly enabled: boolean;
}

// ---------- Forsinkelsesmåling ----------

/**
 * Verten sender `TestDelay` periodisk og forventer den ekkoet tilbake (slik
 * måler den RTT og justerer bitrate). Vi kan også sende våre egne med
 * `fromClient: true`; verten ekkoer dem, og vi måler vår egen forsinkelse.
 */
export interface RdTestDelay {
  readonly time: bigint;
  readonly fromClient: boolean;
  readonly lastDelay: number;
  readonly targetBitrate: number;
}

// ---------- Skjermbytte ----------

/**
 * Går begge veier under `Misc.switch_display`: klienten ber om bytte ved å
 * sette `display`; verten bekrefter/annonserer med full geometri.
 */
export interface RdSwitchDisplay {
  readonly display: number;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly cursorEmbedded: boolean;
  readonly originalResolution?: RdResolution;
}

// ---------- Markør ----------

/**
 * `colors` er ALLTID zstd-komprimert på ledningen (uten noe flagg i
 * meldingen) og pakker ut til nøyaktig width*height*4 byte ikke-
 * premultiplisert RGBA, rad for rad ovenfra. Verifisert mot vertens
 * markørfangst for Windows/X11/macOS 2026-09-07.
 */
export interface RdCursorData {
  readonly id: bigint;
  readonly hotx: number;
  readonly hoty: number;
  readonly width: number;
  readonly height: number;
  readonly colors: Uint8Array;
}

/** Vertens GLOBALE skjermkoordinater (kan være negative) — ikke skjermlokale. */
export interface RdCursorPosition {
  readonly x: number;
  readonly y: number;
}

// ---------- Innstillinger (OptionMessage) ----------

/**
 * MERK FELTVERDIENE. `BoolOption` er IKKE en boolsk verdi: 0 = «ikke satt»,
 * 1 = NEI, 2 = JA. Den intuitive-men-gale koblingen (1 = ja) sender altså et
 * eksplisitt NEI, og et eksplisitt nei er ikke det samme som fraværende —
 * verten skiller på dem. Verifisert mot message.proto (nestet i
 * OptionMessage) og mot vertens `q != BoolOption::NotSet`-lesning.
 */
export const BoolOption = {
  NotSet: 0,
  No: 1,
  Yes: 2,
} as const;

/** `ImageQuality` hopper over 1 — verdiene er dem protokollen faktisk bruker. */
export const ImageQuality = {
  NotSet: 0,
  Low: 2,
  Balanced: 3,
  Best: 4,
} as const;

export const PreferCodec = {
  Auto: 0,
  VP9: 1,
  H264: 2,
  H265: 3,
  VP8: 4,
  AV1: 5,
} as const;

/**
 * Hvilke kodeker VI kan dekode. Feltene er `int32` i protokollen, men brukes
 * som 0/1-flagg: verten tester bare `> 0`. Vi sender derfor eksakt 1.
 *
 * To fakta som er lette å ta feil av, begge verifisert mot vertens
 * `Encoder::update`:
 *  - `ability_vp9` LESES ALDRI av verten. VP9 er den hardkodede grunnlinjen,
 *    og det finnes ingen måte å be seg fritatt fra den. En nettleser som ikke
 *    kan dekode VP9 kan altså ikke sikres av protokollen.
 *  - `i444`/`prefer_chroma` utelates bevisst. Referanseklienten sender
 *    `i444 { vp9: true, av1: true }`, men gjør vi det samme, kan en vert med
 *    bare oss som seer sende VP9 profil 1 (4:4:4) — som de fleste
 *    WebCodecs-dekodere avviser, uten noe flagg på ledningen som forklarer det.
 */
export interface RdSupportedDecoding {
  readonly abilityVp8: boolean;
  readonly abilityVp9: boolean;
  readonly abilityAv1: boolean;
  readonly abilityH264: boolean;
  readonly abilityH265: boolean;
  readonly prefer: number;
}

/**
 * Delvis `OptionMessage`. Vi sender bare feltene vi faktisk mener noe med:
 * alt utelatt leses av verten som «ikke satt», og verten har egne standarder
 * for dem. Merk at `Misc.option` har en egen kompatibilitetssjekk hos verten
 * som krever at et `supported_decoding`-oppdrag sendes ALENE — ikke slå
 * sammen kodek-oppdateringer og kvalitetsendringer i én melding.
 */
export interface RdOptionMessage {
  readonly imageQuality?: number;
  readonly showRemoteCursor?: number;
  readonly disableAudio?: number;
  readonly disableClipboard?: number;
  readonly supportedDecoding?: RdSupportedDecoding;
}

// ---------- Tofaktor ----------

export interface RdAuth2FA {
  readonly code: string;
  readonly hwid: Uint8Array;
}

// ---------- Video ----------

export interface RdEncodedVideoFrame {
  readonly data: Uint8Array;
  readonly key: boolean;
  readonly pts: bigint;
}

export type RdVideoCodec = 'vp9' | 'vp8' | 'h264' | 'h265' | 'av1';

export interface RdVideoFrame {
  readonly codec: RdVideoCodec;
  readonly frames: readonly RdEncodedVideoFrame[];
  readonly display: number;
}

// ---------- Rendezvous ----------

export const ConnType = {
  DefaultConn: 0,
  FileTransfer: 1,
  PortForward: 2,
  Rdp: 3,
  ViewCamera: 4,
  Terminal: 5,
} as const;

export const NatType = { Unknown: 0, Asymmetric: 1, Symmetric: 2 } as const;

export interface RdPunchHoleRequest {
  readonly id: string;
  readonly natType: number;
  readonly licenceKey: string;
  readonly connType: number;
  readonly token: string;
  readonly version: string;
  readonly forceRelay: boolean;
}

export const PunchHoleFailure = {
  IdNotExist: 0,
  Offline: 2,
  LicenseMismatch: 3,
  LicenseOveruse: 4,
} as const;

/**
 * Flat, ikke en tagget union: `failure` (felt 3) er IKKE markert som del av
 * en oneof i .proto-kilden, og IdNotExist sin verdi (0) er samtidig proto3
 * sin fraværs-standardverdi for et enum-felt — det finnes altså ingen
 * garantert måte å skille "eksplisitt IdNotExist" fra "failure ikke satt" ut
 * fra skjemaet alene. Kalleren avgjør suksess ved å sjekke om relayServer/pk
 * faktisk er populert, ikke ved å stole blindt på `failure`. Se
 * docs/rustdesk-protocol.md, "Ukjente", for detaljer — dette bør verifiseres
 * mot en reell hbbs-respons før produksjon.
 */
export interface RdPunchHoleResponse {
  readonly relayServer: string;
  readonly pk: Uint8Array;
  readonly failure: number;
  readonly otherFailure: string;
}

export interface RdRequestRelay {
  readonly id: string;
  readonly uuid: string;
  readonly relayServer: string;
  readonly secure: boolean;
  readonly licenceKey: string;
  readonly connType: number;
  readonly token: string;
}

/**
 * `refuseReason` ikke-tom er den eneste utvetydige feilindikatoren her.
 * `peerId` (felt 4) og `pk` (felt 5) er to grener av samme oneof — kun én er
 * satt av avsenderen. `pk` er serverens signerte vouch for motparten når den
 * finnes; se SecureChannel.verifyServerVouch.
 */
export interface RdRelayResponse {
  readonly uuid: string;
  readonly relayServer: string;
  readonly peerId: string;
  readonly pk: Uint8Array;
  readonly refuseReason: string;
  readonly version: string;
}

export interface RdKeyExchange {
  readonly keys: readonly Uint8Array[];
}
