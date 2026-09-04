import type { RemoteError } from '../errors/RemoteError.js';

// ---------- Session state ----------

export type SessionState =
  | 'connecting'
  | 'authenticating'
  | 'connected'
  | 'reconnecting'
  | 'disconnected'
  | 'failed';

export type DisconnectReason =
  | 'user'
  | 'transient-network'
  | 'authentication-failed'
  | 'remote-shutdown'
  | 'permission-revoked'
  | 'relay-failure'
  | 'session-timeout'
  | 'protocol-error'
  | 'unknown';

export type QualityLevel = 'low' | 'medium' | 'high' | 'auto';

// ---------- Displays ----------

export interface RemoteDisplay {
  readonly id: string;
  readonly label: string;
  readonly width: number;
  readonly height: number;
  readonly isPrimary: boolean;
  readonly scaleFactor: number;
}

// ---------- Permissions ----------

export type RemotePermission =
  | 'screen.view'
  | 'input.pointer'
  | 'input.keyboard'
  | 'clipboard.read'
  | 'clipboard.write'
  | 'files.read'
  | 'files.write'
  | 'audio.listen';

// ---------- Actors & actions ----------

export type SessionActor = 'human' | 'ai' | 'system';

export type PointerButton = 'left' | 'right' | 'middle';

interface ActionBase {
  readonly actor: SessionActor;
}

export type RemoteAction =
  | (ActionBase & {
      readonly type: 'pointer.move';
      readonly x: number;
      readonly y: number;
      readonly displayId?: string;
    })
  | (ActionBase & {
      readonly type: 'pointer.click';
      readonly x: number;
      readonly y: number;
      readonly button: PointerButton;
      readonly displayId?: string;
    })
  | (ActionBase & {
      readonly type: 'pointer.doubleClick';
      readonly x: number;
      readonly y: number;
      readonly button: PointerButton;
      readonly displayId?: string;
    })
  | (ActionBase & {
      readonly type: 'pointer.scroll';
      readonly x: number;
      readonly y: number;
      readonly deltaX: number;
      readonly deltaY: number;
      readonly displayId?: string;
    })
  | (ActionBase & { readonly type: 'keyboard.keyDown'; readonly key: string })
  | (ActionBase & { readonly type: 'keyboard.keyUp'; readonly key: string })
  | (ActionBase & { readonly type: 'keyboard.type'; readonly text: string })
  | (ActionBase & { readonly type: 'clipboard.write'; readonly text: string })
  | (ActionBase & { readonly type: 'display.select'; readonly displayId: string });

export type RemoteActionType = RemoteAction['type'];

export interface ActionResult {
  readonly ok: boolean;
  readonly action: RemoteAction;
  readonly error?: RemoteError;
}

export type ActionMiddleware = (
  action: RemoteAction,
  next: () => Promise<ActionResult>,
) => Promise<ActionResult>;

// ---------- Video frames ----------

export interface RemoteVideoFrame {
  readonly width: number;
  readonly height: number;
  readonly timestamp: number;
  readonly displayId?: string;
  readonly bitmap?: ImageBitmap;
  readonly videoFrame?: VideoFrame;
  readonly rgba?: Uint8Array;
}

export interface CaptureRegion {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

export interface CaptureFrameOptions {
  readonly displayId?: string;
  readonly region?: CaptureRegion;
}

// ---------- Stats ----------

export interface SessionStats {
  readonly latencyMs: number;
  readonly fps: number;
  readonly bitrateKbps: number;
  readonly droppedFrames: number;
  readonly decodedFrames: number;
  readonly renderedFrames: number;
}

// ---------- Authentication ----------

/**
 * Formet etter RustDesk sin faktiske login-utfordring: en stabil, persistert
 * `salt` for målenheten pluss en fersk, engangs `challenge` generert per
 * tilkoblingsforsøk (se docs/rustdesk-protocol.md, avsnittet om Hash-meldingen).
 * Feltnavnene er bevisst generiske — et fremtidig, annet backend-protokoll
 * kunne i prinsippet gjenbruke samme utfordring/svar-form.
 */
export interface AuthenticationChallenge {
  readonly deviceId: string;
  readonly salt: string;
  readonly challenge: string;
}

export interface AuthenticationResponse {
  readonly passwordHash: Uint8Array;
}

/**
 * Isolerer legitimasjonshåndtering fra selve protokollimplementasjonen.
 * Verevons backend kan senere levere en autentikator som utsteder
 * kortlevde øktnøkler i stedet for et RustDesk-passord.
 */
export interface SessionAuthenticator {
  authenticate(challenge: AuthenticationChallenge): Promise<AuthenticationResponse>;
}

// ---------- Connect options ----------

export interface ConnectOptions {
  readonly deviceId: string;
  readonly authenticator?: SessionAuthenticator;
  readonly auth?: { readonly token: string };
  readonly signal?: AbortSignal;
}

// ---------- Events ----------

// `extends Record<string, unknown>` gir grensesnittet en indekssignatur slik
// at EventBus<TEventMap extends Record<string, unknown>> aksepterer det —
// uten denne trikset ser TypeScript ikke et fast-nøkkel-grensesnitt som en
// gyldig Record, selv om hvert felt faktisk er tilordnbart til `unknown`.
export interface RemoteEventMap extends Record<string, unknown> {
  readonly state: { readonly state: SessionState; readonly previous: SessionState };
  readonly frame: RemoteVideoFrame;
  readonly 'display-change': { readonly displays: readonly RemoteDisplay[] };
  readonly latency: { readonly latencyMs: number };
  readonly quality: { readonly level: QualityLevel };
  readonly 'permission-change': { readonly permissions: readonly RemotePermission[] };
  readonly clipboard: { readonly text?: string };
  readonly error: RemoteError;
  readonly disconnect: { readonly reason: DisconnectReason };
}

export type RemoteEvent = keyof RemoteEventMap;
export type RemoteEventHandler<T extends RemoteEvent> = (payload: RemoteEventMap[T]) => void;
export type Unsubscribe = () => void;

// ---------- AI observer ----------

export interface AIObserverOptions {
  readonly maxFramesPerSecond: number;
  readonly displayId?: string;
  // Merk: region-beskjæring finnes på CaptureFrameOptions (ett-gangs
  // skjermbilder), men er IKKE implementert for det kontinuerlige
  // observer-strømmen ennå — det krever beskjæringslogikk som avhenger av
  // hvilken frame-representasjon videodekoderen faktisk produserer
  // (rgba/bitmap/VideoFrame), som først avgjøres når kodek-pipelinen bygges.
  // Feltet er bevisst utelatt her i stedet for tatt imot og ignorert.
}

export interface AIObserverEventMap extends Record<string, unknown> {
  readonly frame: RemoteVideoFrame;
}

export interface AIObserver {
  on<T extends keyof AIObserverEventMap>(
    event: T,
    handler: (payload: AIObserverEventMap[T]) => void,
  ): Unsubscribe;
  pause(): void;
  resume(): void;
  readonly isPaused: boolean;
  close(): void;
}

// ---------- Controllers ----------

export interface PointerController {
  move(point: { x: number; y: number; displayId?: string }): Promise<ActionResult>;
  click(options: { button: PointerButton; x?: number; y?: number; displayId?: string }): Promise<ActionResult>;
  doubleClick(options: {
    button: PointerButton;
    x?: number;
    y?: number;
    displayId?: string;
  }): Promise<ActionResult>;
  scroll(delta: { deltaX: number; deltaY: number; x?: number; y?: number; displayId?: string }): Promise<ActionResult>;
}

export interface KeyboardController {
  keyDown(key: string): Promise<ActionResult>;
  keyUp(key: string): Promise<ActionResult>;
  type(text: string): Promise<ActionResult>;
}

export interface ClipboardController {
  write(text: string): Promise<ActionResult>;
  readonly lastKnownText: string | undefined;
}

export interface PermissionController {
  has(permission: RemotePermission): boolean;
  list(): readonly RemotePermission[];
  onChange(handler: (permissions: readonly RemotePermission[]) => void): Unsubscribe;
}

export interface ActionsController {
  execute(action: RemoteAction): Promise<ActionResult>;
  use(middleware: ActionMiddleware): void;
}

export interface AIController {
  createObserver(options: AIObserverOptions): AIObserver;
}

export interface StatsController {
  getSnapshot(): SessionStats;
}

// ---------- Session & client ----------

export interface RemoteSession {
  readonly id: string;
  readonly state: SessionState;
  readonly displays: readonly RemoteDisplay[];
  readonly pointer: PointerController;
  readonly keyboard: KeyboardController;
  readonly clipboard: ClipboardController;
  readonly permissions: PermissionController;
  readonly actions: ActionsController;
  readonly ai: AIController;
  readonly stats: StatsController;

  captureFrame(options?: CaptureFrameOptions): Promise<RemoteVideoFrame>;
  disconnect(): Promise<void>;
  on<T extends RemoteEvent>(event: T, handler: RemoteEventHandler<T>): Unsubscribe;
}

export interface RemoteClient {
  connect(options: ConnectOptions): Promise<RemoteSession>;
}
