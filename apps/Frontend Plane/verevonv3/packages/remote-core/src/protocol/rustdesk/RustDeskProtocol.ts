import type {
  ActionResult,
  CaptureFrameOptions,
  QualityLevel,
  RemoteAction,
  RemoteCursorShape,
  RemoteDisplay,
  RemotePermission,
  RemoteVideoFrame,
  Unsubscribe,
} from '../../types/public.js';
import type { InternalSessionState, ProtocolEvent, ProtocolEventMap } from '../../types/internal.js';
import type { ProtocolConnectOptions, RemoteProtocol } from '../RemoteProtocol.js';
import type { Transport } from '../../transport/Transport.js';
import type { Logger } from '../../logging/Logger.js';
import { noopLogger } from '../../logging/Logger.js';
import { EventBus } from '../../events/EventBus.js';
import { WebSocketTransport } from '../../transport/WebSocketTransport.js';
import {
  AuthenticationError,
  CodecError,
  ProtocolError,
  RemoteConnectionError,
  TransportError,
} from '../../errors/RemoteError.js';
import { cloneVideoFrame, cropVideoFrame, releaseVideoFrame } from '../../media/VideoFrame.js';
import { throwIfAborted, withAbort, withTimeout } from '../abort.js';
import { Decompress as ZstdDecompress } from 'fzstd';
import { RemoteVideoDecoder, detectMediaCapabilities, type MediaCapabilities } from '../../media/RemoteVideoDecoder.js';
import { mapBrowserKeyToIdentification, mapTextToIdentification } from '../../input/KeyboardMapper.js';
import { MessageInbox } from './MessageInbox.js';
import { RendezvousClient } from './RendezvousClient.js';
import { SecureChannel, verifyServerVouch, type PeerIdentity } from './SecureChannel.js';
import { decodeMessage, encodeMessage, encodeRendezvousMessage, type IncomingMessage, type OutgoingMessage } from './messages/envelope.js';
import {
  BoolOption,
  ClipboardFormat,
  ConnType,
  ImageQuality,
  KeyboardMode,
  MouseButtonFlag,
  MouseEventKind,
  Permission,
  PreferCodec,
  type RdDisplayInfo,
  type RdPeerInfo,
  type RdPermissionInfo,
  type RdCursorData,
  type RdCursorPosition,
  type RdOptionMessage,
  type RdSupportedDecoding,
  type RdSwitchDisplay,
  type RdTestDelay,
} from './messages/types.js';

export interface RustDeskProtocolOptions {
  /** wss://…  — hbbs sitt WebSocket-endepunkt (port 21118 bak TLS-terminering). */
  readonly rendezvousUrl: string;
  /** wss://… — hbbr. Overstyrer adressen serveren oppgir; nødvendig når hbbr står bak en egen TLS-proxy. */
  readonly relayUrl?: string;
  /**
   * Base64-kodet Ed25519 offentlig nøkkel for rendezvous-serveren. Uten denne
   * kan vi ikke verifisere hvem motparten er — se `allowUnverifiedPeer`.
   */
  readonly serverPublicKey?: string;
  /**
   * Tilgangsnøkkelen hbbs krever (`-k`/`KEY`). I et vanlig RustDesk-oppsett er
   * dette SAMME streng som `serverPublicKey`: hbbs genererer ett
   * Ed25519-par, logger den offentlige nøkkelen som «Key: …», og klienter
   * oppgir den både som tilgangsnøkkel og til signaturverifisering.
   * Verifisert mot en kjørende hbbs — utelates den mens `serverPublicKey` er
   * satt, gjenbrukes sistnevnte, siden alternativet er et forvirrende
   * «license key mismatch» på et ellers riktig oppsett.
   */
  readonly licenceKey?: string;
  readonly token?: string;
  readonly clientVersion?: string;
  readonly clientId?: string;
  readonly clientName?: string;
  /** Se SecureChannel: kun for lokal utvikling mot en nøkkelløs testserver. */
  readonly allowUnverifiedPeer?: boolean;
  /**
   * Gir input.pointer/input.keyboard umiddelbart ved innlogging i stedet for å
   * vente på en `PermissionInfo`-melding.
   *
   * Hvorfor dette finnes: forskningen fikk IKKE bekreftet om verten sender et
   * fullt tillatelsessett rett etter innlogging, eller bare deltaer når noe
   * endres (docs/rustdesk-protocol.md, "Ukjente"). Er det sistnevnte, vil en
   * klient som venter på eksplisitte hendelser aldri få input-tillatelse, og
   * all styring blir avvist. Standard er `false` (trygt: ingenting antas),
   * men da kan input være blokkert mot en ekte vert til dette er verifisert.
   */
  readonly assumeInputPermittedOnLogin?: boolean;
  readonly logger?: Logger;
  /** Injiseres i tester for å unngå en ekte WebSocket. */
  readonly createTransport?: (url: string) => Transport;
  readonly rendezvousTimeoutMs?: number;
  readonly loginTimeoutMs?: number;
  /**
   * Hvor ofte vi sender vår egen `TestDelay`-sonde for å måle RTT fra
   * nettleserens side. Verten sender uansett sine egne sonder (~1/s) som vi
   * ekkoer, og oppgir sin siste måling i dem — begge kilder ender i
   * 'latency'-hendelser. 0 slår av våre sonder. Standard: 2000 ms.
   */
  readonly latencyProbeIntervalMs?: number;
  /**
   * Hvor lenge vi venter på at noen faktisk oppgir tofaktorkoden. Uten en
   * grense henger connect() for alltid med et åpent relé hvis operatøren går
   * fra maskinen. Standard: samme som `loginTimeoutMs`.
   */
  readonly secondFactorTimeoutMs?: number;
  /**
   * Hvilken kodek vi ber verten foretrekke. Standard 'vp9': det er den eneste
   * kodeken protokollen garanterer (verten leser aldri `ability_vp9` og faller
   * alltid tilbake til VP9), og den har programvaredekoder i alle nettlesere
   * med WebCodecs. 'h264' kan gi maskinvaredekoding, men RustDesk koder H264
   * KUN med maskinvare-enkoder, så verten kan ignorere ønsket.
   */
  readonly preferredCodec?: 'auto' | 'vp9' | 'h264';
  /**
   * Overstyrer nettleser-sonderingen av dekoderstøtte. Injiseres i tester
   * (Node har ingen WebCodecs) og av kallere som allerede vet svaret.
   */
  readonly mediaCapabilities?: MediaCapabilities;
}

/**
 * RustDesk sitt tillatelses-enum dekker ikke helt vårt offentlige sett. Merk
 * spesielt at RustDesk gater BÅDE mus og tastatur bak ett `Keyboard`-flagg —
 * det finnes ingen separat mus-tillatelse i protokollen.
 * Restart/Recording/BlockInput/PrivacyMode har ingen offentlig motpart hos oss
 * og ignoreres bevisst i stedet for å bli oversatt til noe de ikke betyr.
 */
const PERMISSION_MAP: Readonly<Record<number, readonly RemotePermission[]>> = {
  [Permission.Keyboard]: ['input.keyboard', 'input.pointer'],
  [Permission.Clipboard]: ['clipboard.read', 'clipboard.write'],
  [Permission.Audio]: ['audio.listen'],
  [Permission.File]: ['files.read', 'files.write'],
};

/** Tilstander der en ny connect() ville trampet på et pågående/levende forsøk. */
const ACTIVE_STATES: ReadonlySet<InternalSessionState> = new Set([
  'connecting',
  'rendezvous',
  'authenticating',
  'connected',
  'reconnecting',
]);

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/**
 * `isPrimary` betyr her "skjermen verten strømmer akkurat nå" — det er den
 * eneste skjerm-markøren RustDesk faktisk formidler (`PeerInfo.current_display`
 * og senere `SwitchDisplay.display`); protokollen har ingen egen
 * «hovedskjerm»-flagg.
 */
function toRemoteDisplays(peerInfo: RdPeerInfo): readonly RemoteDisplay[] {
  return peerInfo.displays.map((display: RdDisplayInfo, index: number) => ({
    id: String(index),
    label: display.name || `Display ${index + 1}`,
    width: display.width,
    height: display.height,
    isPrimary: index === peerInfo.currentDisplay,
    scaleFactor: display.scale > 0 ? display.scale : 1,
    cursorEmbedded: display.cursorEmbedded,
  }));
}

interface DisplayOrigin {
  readonly x: number;
  readonly y: number;
}

/**
 * Markørformer verten kan minte en ny id for per animasjonsbilde; verten
 * begrenser sin egen sende-cache til 64 på Linux/DRM, så samme tak her holder
 * minnet bundet uten å miste former som faktisk er i bruk.
 */
const MAX_CURSOR_CACHE = 64;
/** Windows-markører er ≤ 256 px; 1024 gir rom for HiDPI uten å tillate allokeringsbomber. */
const MAX_CURSOR_DIMENSION = 1024;

/**
 * Verten har bekreftet et skjermbytte: flytt markøren og ta imot ny geometri.
 * Returnerer `undefined` når verten oppgir en skjerm vi aldri fikk se i
 * `PeerInfo` — da er skjermlisten vår foreldet, og å bare `map()`-e over den
 * ville fjernet markøren fra ALLE skjermer uten å si fra.
 */
function applySwitchDisplay(
  displays: readonly RemoteDisplay[],
  switched: RdSwitchDisplay,
): readonly RemoteDisplay[] | undefined {
  if (switched.display < 0 || switched.display >= displays.length) return undefined;
  return displays.map((display, index) => {
    if (index !== switched.display) {
      return display.isPrimary ? { ...display, isPrimary: false } : display;
    }
    return {
      ...display,
      isPrimary: true,
      // 0 betyr «ikke oppgitt» i proto3 — behold det vi visste fra PeerInfo.
      width: switched.width > 0 ? switched.width : display.width,
      height: switched.height > 0 ? switched.height : display.height,
      cursorEmbedded: switched.cursorEmbedded,
    };
  });
}

/**
 * Pakker ut zstd til NØYAKTIG `expectedLength` byte. Strømmende, slik at vi
 * kan stanse i samme øyeblikk utdataene overstiger det deklarerte — en
 * løgnaktig rammehode-størrelse får da aldri allokert noe — og slik at et
 * for KORT resultat også avvises (fzstd sin engangs-`decompress` med
 * forhåndsallokert buffer returnerer hele bufferet, null-utfylt, og skjuler
 * det). Kaster ved begge avvik.
 */
function decompressExactly(input: Uint8Array, expectedLength: number): Uint8Array {
  const out = new Uint8Array(expectedLength);
  let written = 0;
  const decompressor = new ZstdDecompress((chunk) => {
    if (written + chunk.length > expectedLength) {
      throw new Error(`decompressed data exceeds the declared ${expectedLength} bytes`);
    }
    out.set(chunk, written);
    written += chunk.length;
  });
  decompressor.push(input, true);
  if (written !== expectedLength) {
    throw new Error(`decompressed ${written} bytes, expected ${expectedLength}`);
  }
  return out;
}

/** RustDesk sitt eget vokabular for «passord OK, men verten krever TOTP». */
const LOGIN_ERROR_2FA_REQUIRED = '2FA Required';

/**
 * Over dette regner vi en RTT som oppdiktet heller enn treg. Verten kobler
 * selv ned etter 30 s uten trafikk, så en ekte måling kan ikke overstige det.
 */
const MAX_PLAUSIBLE_LATENCY_MS = 30_000;

/**
 * `ImageQuality` har ingen 'auto'-verdi. NotSet er nettopp «bestem selv», som
 * er vertens standardoppførsel (den kjører sin egen ABR-løkke).
 */
const IMAGE_QUALITY_BY_LEVEL: Readonly<Record<QualityLevel, number>> = {
  low: ImageQuality.Low,
  medium: ImageQuality.Balanced,
  high: ImageQuality.Best,
  auto: ImageQuality.NotSet,
};

/**
 * Den eneste klassen som kjenner hele RustDesk-sekvensen. Implementerer
 * RemoteProtocol, slik at RemoteClient/RemoteSession og hele den offentlige
 * API-en er uvitende om at RustDesk finnes.
 *
 * VERIFISERINGSSTATUS (se docs/architecture.md, "Status", for detaljer):
 *  - Rendezvous-halvdelen er kjørt mot en ekte hbbs: den godtar vår
 *    PunchHoleRequest og vi dekoder svaret riktig.
 *  - Resten (relé-paring, håndtrykk, innlogging, øktstrøm) er verifisert mot
 *    FakeRustDeskPeer, ikke mot en ekte vert — det krever en registrert
 *    Verevon Agent, som ikke finnes ennå. Regn derfor ikke hele sekvensen
 *    som produksjonsklar.
 */
export class RustDeskProtocol implements RemoteProtocol {
  private readonly bus = new EventBus<ProtocolEventMap>();
  private inbox = new MessageInbox<IncomingMessage>();
  private readonly logger: Logger;
  private readonly createTransport: (url: string) => Transport;

  private state: InternalSessionState = 'idle';
  private relayTransport: Transport | undefined;
  private secureChannel: SecureChannel | undefined;
  private decoder: RemoteVideoDecoder | undefined;
  private retainedFrame: RemoteVideoFrame | undefined;
  private latencyProbe: ReturnType<typeof setInterval> | undefined;
  private capabilities: MediaCapabilities | undefined;
  /** Nivået vi har BEDT om. 'auto' betyr at vertens egen ABR styrer. */
  private requestedQuality: QualityLevel = 'auto';
  private lastReportedBitrateKbps: number | undefined;
  private readonly cursorCache = new Map<bigint, RemoteCursorShape>();
  private displayOrigins: readonly DisplayOrigin[] = [];
  private currentDisplayIndex = 0;
  private readonly unsubscribers: Unsubscribe[] = [];

  private _displays: readonly RemoteDisplay[] = [];
  private _permissions: readonly RemotePermission[] = [];

  // Secretbox-nonce er en teller: både kryptering og dekryptering MÅ skje i
  // streng rekkefølge. Disse to kjedene serialiserer begge retninger, slik at
  // to samtidige kall ikke kan bytte om på rekkefølgen og desynkronisere
  // telleren.
  private inboundChain: Promise<void> = Promise.resolve();
  private outboundChain: Promise<void> = Promise.resolve();

  constructor(private readonly options: RustDeskProtocolOptions) {
    this.logger = options.logger ?? noopLogger;
    this.createTransport = options.createTransport ?? ((url) => new WebSocketTransport({ url }));
  }

  get displays(): readonly RemoteDisplay[] {
    return this._displays;
  }

  get permissions(): readonly RemotePermission[] {
    return this._permissions;
  }

  on<T extends ProtocolEvent>(event: T, handler: (payload: ProtocolEventMap[T]) => void): Unsubscribe {
    return this.bus.on(event, handler);
  }

  async connect(options: ProtocolConnectOptions): Promise<void> {
    if (ACTIVE_STATES.has(this.state)) {
      throw new ProtocolError(`connect() called while the protocol is in state "${this.state}"`);
    }
    // Et tidligere mislykket forsøk skal ikke gjøre instansen ubrukelig for
    // godt: rydd opp og start fra en frisk innboks.
    this.resetForConnect();

    try {
      await this.runConnect(options);
    } catch (error) {
      await this.abandonConnect();
      throw error;
    }
  }

  private async runConnect(options: ProtocolConnectOptions): Promise<void> {
    const rendezvousTimeoutMs = this.options.rendezvousTimeoutMs ?? 15_000;
    const loginTimeoutMs = this.options.loginTimeoutMs ?? 30_000;
    const clientVersion = this.options.clientVersion ?? '1.4.9';
    const signal = options.signal;

    throwIfAborted(signal);
    this.transition('connecting');

    // --- 1. Rendezvous: forhandle fram relé ---
    this.transition('rendezvous');
    const rendezvous = new RendezvousClient(this.createTransport(this.options.rendezvousUrl), this.logger);
    let negotiation;
    try {
      negotiation = await withAbort(
        rendezvous.negotiateRelay({
          deviceId: options.deviceId,
          licenceKey: this.effectiveLicenceKey(),
          token: this.options.token ?? '',
          clientVersion,
          timeoutMs: rendezvousTimeoutMs,
        }),
        signal,
      );
    } finally {
      // Rendezvous-forbindelsen har gjort sitt uansett utfall.
      await rendezvous.close().catch(() => undefined);
    }

    const peerIdentity = await this.resolvePeerIdentity(negotiation.peerVouch);

    // --- 2. Relé: koble til hbbr og bli paret med motparten ---
    const relayUrl = this.options.relayUrl ?? negotiation.relayServer;
    if (!relayUrl) {
      throw new RemoteConnectionError(
        'No relay address available: the server returned none and no relayUrl was configured',
      );
    }

    throwIfAborted(signal);
    const relay = this.createTransport(relayUrl);
    await withAbort(relay.connect(), signal);
    this.relayTransport = relay;
    this.wireRelayTransport(relay);

    // hbbr sin FØRSTE melding må være RequestRelay med økt-tokenet; etter
    // paring er hbbr bare en byte-pipe og ser aldri protokollen vår igjen.
    relay.send(
      encodeRendezvousMessage({
        kind: 'requestRelay',
        value: {
          id: options.deviceId,
          uuid: negotiation.uuid,
          relayServer: negotiation.relayServer,
          secure: true,
          licenceKey: this.effectiveLicenceKey(),
          connType: ConnType.DefaultConn,
          token: this.options.token ?? '',
        },
      }),
    );

    // --- 3. Ende-til-ende-håndtrykk med motparten ---
    this.transition('authenticating');
    this.secureChannel = new SecureChannel({
      peerIdentity,
      allowUnverifiedPeer: this.options.allowUnverifiedPeer,
      logger: this.logger,
    });

    const signedId = await withAbort(this.expect('signedId', loginTimeoutMs), signal);
    const publicKey = await this.secureChannel.acceptSignedId(signedId.value);
    // Denne siste meldingen går i KLARTEKST — kanalen er kryptert først etter
    // at motparten har fått øktnøkkelen vi forsegler her.
    relay.send(encodeMessage({ kind: 'publicKey', value: publicKey }));

    // --- 4. Innlogging (kryptert fra nå) ---
    const hash = await withAbort(this.expect('hash', loginTimeoutMs), signal);
    const authentication = await withAbort(
      options.authenticator.authenticate({
        deviceId: options.deviceId,
        salt: hash.value.salt,
        challenge: hash.value.challenge,
      }),
      signal,
    );

    await this.sendEncrypted({
      kind: 'loginRequest',
      value: {
        username: options.deviceId,
        password: authentication.passwordHash,
        myId: this.options.clientId ?? 'verevon-web',
        myName: this.options.clientName ?? 'Verevon Support',
        myPlatform: 'Web',
        sessionId: randomSessionId(),
        version: clientVersion,
        videoAckRequired: false,
        hwid: new Uint8Array(0),
        avatar: '',
        option: await this.buildLoginOption(),
      },
    });

    let result = (await withAbort(this.expect('loginResponse', loginTimeoutMs), signal)).value.result;

    // --- 4b. Andre faktor (kun når verten ber om det) ---
    // Verten svarer «2FA Required» ETTER at passordet er godkjent, og venter
    // så på én Auth2FA på samme krypterte kanal før den enten sender PeerInfo
    // eller «Wrong 2FA Code».
    if (result.kind === 'error' && result.error === LOGIN_ERROR_2FA_REQUIRED) {
      const provideSecondFactor = options.authenticator.provideSecondFactor?.bind(options.authenticator);
      if (!provideSecondFactor) {
        throw new AuthenticationError(
          'The remote device requires two-factor authentication, but no second-factor provider was configured (SessionAuthenticator.provideSecondFactor / auth.secondFactor)',
          { reason: result.error },
        );
      }
      if (options.isAutomaticRetry === true) {
        // En automatisk gjenoppkobling skal ikke sprette opp en kodedialog
        // operatøren ikke ba om — og koden fra forrige forsøk er uansett
        // brukt opp (TOTP er engangs).
        throw new AuthenticationError(
          'Reconnecting to this device needs a fresh two-factor code — reconnect manually to enter one',
          { reason: result.error },
        );
      }

      const code = await withAbort(
        withTimeout(
          provideSecondFactor(),
          this.options.secondFactorTimeoutMs ?? loginTimeoutMs,
          () => new AuthenticationError('Timed out waiting for the two-factor code'),
        ),
        signal,
      );
      // Tom hwid = «ikke husk denne enheten»; vi persisterer ingen
      // enhetsidentitet i nettleseren.
      await this.sendEncrypted({ kind: 'auth2fa', value: { code, hwid: new Uint8Array(0) } });
      result = (await withAbort(this.expect('loginResponse', loginTimeoutMs), signal)).value.result;

      // Et NYTT «2FA Required» betyr at koden ble avvist og verten utfordrer
      // på nytt (typisk et utløpt TOTP-vindu). Uten denne grenen hadde vi
      // rapportert det med samme setning som «ingen kodeleverandør fantes»,
      // og operatøren — som nettopp tastet en kode — kunne ikke se forskjell.
      if (result.kind === 'error' && result.error === LOGIN_ERROR_2FA_REQUIRED) {
        throw new AuthenticationError(
          'The remote device re-issued the two-factor challenge — the code was likely expired. Try again with a fresh code.',
          { reason: result.error },
        );
      }
    }

    if (result.kind === 'error') {
      throw new AuthenticationError(describeLoginError(result.error), { reason: result.error });
    }

    // --- 5. Tilkoblet ---
    this.applyPeerInfo(result.peerInfo);
    this.startDecoder();
    this.transition('connected');
    this.startLatencyProbe();
  }

  /**
   * `LoginRequest.option` (felt 6). Vi setter bare det vi faktisk mener noe
   * med; alt annet leses av verten som «ikke satt», og den har egne
   * standarder for dem.
   *
   *  - `supported_decoding`: hva nettleseren virkelig kan dekode. Utelater vi
   *    hele option-meldingen, registrerer verten oss som «bare VP9» — trygt,
   *    men vi mister muligheten til å be om noe annet.
   *  - `disable_audio: Yes`: vi implementerer ikke lyd. Uten dette abonnerer
   *    verten på lydtjenesten som standard og Opus-koder en strøm vi kaster.
   *  - `show_remote_cursor: Yes`: uten dette sender verten ALDRI
   *    `CursorPosition` (markørFORMER kommer likevel når tastatur er tillatt),
   *    så en tegnet markør ville aldri flyttet seg.
   *  - `disable_clipboard` settes ikke: utklippstavle ER implementert.
   */
  private async buildLoginOption(): Promise<RdOptionMessage> {
    const supportedDecoding = await this.buildSupportedDecoding();
    return {
      disableAudio: BoolOption.Yes,
      showRemoteCursor: BoolOption.Yes,
      // Utelates (ikke sendes tom) når nettleseren ikke kan dekode noe: hvordan
      // verten tolker en TOM submelding er uavklart, mens et fraværende felt
      // dokumentert gir «bare VP9». Se docs/rustdesk-protocol.md.
      ...(supportedDecoding ? { supportedDecoding } : {}),
    };
  }

  private async buildSupportedDecoding(): Promise<RdSupportedDecoding | undefined> {
    this.capabilities ??= this.options.mediaCapabilities ?? (await detectMediaCapabilities());
    const codecs = new Set(this.capabilities.videoCodecs);
    if (codecs.size === 0) {
      this.logger.warn('This browser reports no WebCodecs decode support at all — video will not decode');
      return undefined;
    }

    if (!codecs.has('vp9')) {
      // Verten leser aldri ability_vp9 og faller alltid tilbake til VP9, så
      // dette er ikke noe vi kan forhandle oss ut av — bare varsle om.
      this.logger.warn(
        'This browser reports no VP9 decode support, but the RustDesk protocol always falls back to VP9 — video may not decode',
      );
    }

    const prefer = ((): number => {
      const wanted = this.options.preferredCodec ?? 'vp9';
      if (wanted === 'h264' && codecs.has('h264')) return PreferCodec.H264;
      if (wanted === 'auto') return PreferCodec.Auto;
      // Standard: be eksplisitt om VP9. Da er submeldingen dessuten aldri tom,
      // og vi slipper spørsmålet om hvordan verten tolker et tomt felt.
      return codecs.has('vp9') ? PreferCodec.VP9 : PreferCodec.Auto;
    })();

    return {
      abilityVp8: codecs.has('vp8'),
      abilityVp9: codecs.has('vp9'),
      abilityAv1: codecs.has('av1'),
      abilityH264: codecs.has('h264'),
      abilityH265: codecs.has('h265'),
      prefer,
    };
  }

  /** Se doc-kommentaren på `licenceKey`: samme streng som serverens nøkkel i normaloppsett. */
  private effectiveLicenceKey(): string {
    return this.options.licenceKey ?? this.options.serverPublicKey ?? '';
  }

  private resetForConnect(): void {
    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    // Innboksen er permanent avvisende etter close(), så et nytt forsøk
    // trenger en ny.
    this.inbox = new MessageInbox<IncomingMessage>();
    this.inboundChain = Promise.resolve();
    this.outboundChain = Promise.resolve();
    this.secureChannel = undefined;
    this.relayTransport = undefined;
    this._displays = [];
    this._permissions = [];
    this.lastReportedBitrateKbps = undefined;
    // Id-ene er vertens egne håndtak og gjelder bare for én økt.
    this.cursorCache.clear();
    this.displayOrigins = [];
    this.currentDisplayIndex = 0;
    // Ellers kunne captureFrame() rett etter en gjenoppkobling levert piksler
    // fra FØR bruddet som om de var ferske — og en AI-forbruker handlet på
    // en skjerm som ikke finnes lenger.
    this.releaseRetainedFrame();
  }

  private releaseRetainedFrame(): void {
    if (!this.retainedFrame) return;
    releaseVideoFrame(this.retainedFrame);
    this.retainedFrame = undefined;
  }

  /** Rydder opp etter et mislykket connect() og lander i 'failed'. */
  private async abandonConnect(): Promise<void> {
    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    this.inbox.close(new TransportError('Connect attempt abandoned'));
    this.stopLatencyProbe();
    this.decoder?.close();
    this.decoder = undefined;
    this.releaseRetainedFrame();
    await this.relayTransport?.close().catch(() => undefined);
    this.relayTransport = undefined;
    this.secureChannel = undefined;
    this.transition('failed');
  }

  private async resolvePeerIdentity(peerVouch: Uint8Array): Promise<PeerIdentity | undefined> {
    if (peerVouch.length === 0) {
      this.logger.warn('Rendezvous server sent no signed peer key');
      return undefined;
    }
    const serverKey = this.options.serverPublicKey;
    if (!serverKey) {
      this.logger.warn('No serverPublicKey configured — cannot verify the peer vouch');
      return undefined;
    }
    return verifyServerVouch(peerVouch, decodeBase64(serverKey));
  }

  private wireRelayTransport(relay: Transport): void {
    this.unsubscribers.push(
      relay.onMessage((data) => this.enqueueInbound(data)),
      relay.onClose((info) => {
        this.handleRelayLoss(
          info.wasClean ? 'remote-shutdown' : 'relay-failure',
          new TransportError('Relay connection closed', { code: info.code }),
        );
      }),
      relay.onError(() => {
        this.handleRelayLoss('relay-failure', new TransportError('Relay transport error'));
      }),
    );
  }

  /**
   * Reléet forsvant under oss. Midt i connect() rekker det å lukke innboksen
   * — runConnect() feiler da på sin egen `expect()` og abandonConnect()
   * rydder. I 'connected' rydder vi selv og lander i 'disconnected', slik at
   * et nytt connect() (gjenoppkobling) faktisk er tillatt etterpå. Merk
   * rekkefølgen: 'disconnect' sendes FØR tilstandsovergangen, så en lytter
   * kan klassifisere årsaken før den ser sluttilstanden.
   */
  private handleRelayLoss(reason: 'remote-shutdown' | 'relay-failure', error: TransportError): void {
    this.inbox.close(error);
    if (this.state === 'authenticating') {
      this.bus.emit('disconnect', { reason });
      return;
    }
    if (this.state !== 'connected') return;

    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    this.stopLatencyProbe();
    this.decoder?.close();
    this.decoder = undefined;
    this.releaseRetainedFrame();
    this.relayTransport = undefined;
    this.secureChannel = undefined;
    this.bus.emit('disconnect', { reason });
    this.transition('disconnected');
  }

  /** Serialiserer innkommende trafikk: dekryptering er rekkefølgeavhengig. */
  private enqueueInbound(data: Uint8Array): void {
    this.inboundChain = this.inboundChain
      .then(async () => {
        const channel = this.secureChannel;
        const plaintext = channel?.isEncrypted === true ? await channel.decrypt(data) : data;
        this.handleMessage(decodeMessage(plaintext));
      })
      .catch((error: unknown) => {
        this.logger.error('Failed to process an inbound message', {
          error: error instanceof Error ? error.message : String(error),
        });
        this.inbox.close(error instanceof Error ? error : new ProtocolError('Inbound processing failed'));
      });
  }

  private handleMessage(message: IncomingMessage): void {
    switch (message.kind) {
      case 'videoFrame': {
        // Bytes måles FØR dekoding: det er dette som faktisk kom over linjen,
        // uavhengig av om dekoderen klarte å bruke det.
        let encodedBytes = 0;
        for (const chunk of message.value.frames) encodedBytes += chunk.data.length;
        this.decoder?.decode(message.value);
        this.bus.emit('media-stats', {
          encodedBytes,
          droppedFramesTotal: this.decoder?.droppedBeforeKeyframe ?? 0,
        });
        return;
      }
      case 'permissionInfo':
        this.applyPermissionInfo(message.value);
        return;
      case 'switchDisplay': {
        const next = applySwitchDisplay(this._displays, message.value);
        if (!next) {
          // Verten strømmer en skjerm vi aldri fikk oppgitt. Vi later ikke som
          // vi forsto det: skjermlisten står urørt og hendelsen logges.
          this.logger.warn('Host confirmed a switch to a display we never enumerated', {
            display: message.value.display,
            knownDisplays: this._displays.length,
          });
          return;
        }
        this._displays = next;
        this.currentDisplayIndex = message.value.display;
        this.displayOrigins = this.displayOrigins.map((origin, index) =>
          index === message.value.display ? { x: message.value.x, y: message.value.y } : origin,
        );
        // Referansebildene i dekoderen tilhører forrige skjerm — krev et nytt
        // keyframe før vi dekoder noe mer (verten sender et uoppfordret, fordi
        // den starter enkoderen på nytt når abonnementet flyttes).
        this.decoder?.resetForNewStream();
        this.bus.emit('display-change', { displays: this._displays });
        return;
      }
      case 'testDelay':
        this.handleTestDelay(message.value);
        return;
      case 'cursorData':
        this.handleCursorData(message.value);
        return;
      case 'cursorId':
        this.handleCursorId(message.value);
        return;
      case 'cursorPosition':
        this.handleCursorPosition(message.value);
        return;
      case 'clipboard': {
        const clipboard = message.value;
        if (clipboard.format === ClipboardFormat.Text) {
          this.bus.emit('clipboard', { text: new TextDecoder().decode(clipboard.content) });
        }
        return;
      }
      case 'unknown':
        this.logger.debug('Ignoring an undecoded peer message', { fieldNumber: message.fieldNumber });
        return;
      default:
        // Håndtrykk-/innloggingsmeldinger konsumeres av connect() via inbox.
        this.inbox.push(message);
    }
  }

  private expect<K extends IncomingMessage['kind']>(
    kind: K,
    timeoutMs: number,
  ): Promise<Extract<IncomingMessage, { kind: K }>> {
    return this.inbox.next(
      (message) => message.kind === kind,
      timeoutMs,
      () =>
        new RemoteConnectionError(`Timed out waiting for the peer's "${kind}" message`, { timeoutMs }),
    ) as Promise<Extract<IncomingMessage, { kind: K }>>;
  }

  /** Serialiserer utgående trafikk: nonce-telleren tildeles ved kryptering. */
  private sendEncrypted(message: OutgoingMessage): Promise<void> {
    const send = async (): Promise<void> => {
      const channel = this.secureChannel;
      const relay = this.relayTransport;
      if (!channel || !relay) {
        throw new ProtocolError('Cannot send: the session is not established');
      }
      relay.send(await channel.encrypt(encodeMessage(message)));
    };
    this.outboundChain = this.outboundChain.then(send, send);
    return this.outboundChain;
  }

  private applyPeerInfo(peerInfo: RdPeerInfo): void {
    this._displays = toRemoteDisplays(peerInfo);
    // CursorPosition kommer i vertens globale koordinater; opprinnelsen per
    // skjerm er det som gjør dem skjermlokale.
    this.displayOrigins = peerInfo.displays.map((display) => ({ x: display.x, y: display.y }));
    this.currentDisplayIndex = peerInfo.currentDisplay;
    this.bus.emit('display-change', { displays: this._displays });

    // Innlogging lyktes, så vi ser faktisk skjermen. Styringstillatelser
    // kommer separat — "Never assume control permissions from screen-view".
    const granted = new Set<RemotePermission>(['screen.view']);
    if (this.options.assumeInputPermittedOnLogin === true) {
      granted.add('input.pointer');
      granted.add('input.keyboard');
    }
    this._permissions = [...granted];
    this.bus.emit('permission-change', { permissions: this._permissions });
  }

  private applyPermissionInfo(info: RdPermissionInfo): void {
    const mapped = PERMISSION_MAP[info.permission];
    if (!mapped) {
      this.logger.debug('Ignoring a permission with no public equivalent', { permission: info.permission });
      return;
    }
    const next = new Set(this._permissions);
    for (const permission of mapped) {
      if (info.enabled) next.add(permission);
      else next.delete(permission);
    }
    this._permissions = [...next];
    this.bus.emit('permission-change', { permissions: this._permissions });
  }

  private startDecoder(): void {
    this.decoder = new RemoteVideoDecoder({
      logger: this.logger,
      onFrame: (frame) => {
        // Behold en egen kopi for captureFrame(); originalen eies av
        // RemoteSession, som frigjør den etter utsending.
        if (this.retainedFrame) releaseVideoFrame(this.retainedFrame);
        this.retainedFrame = cloneVideoFrame(frame);
        this.bus.emit('frame', frame);
      },
      onError: (error) => {
        this.logger.error('Video decode failed', { message: error.message });
        // Uten dette var en permanent død videostrøm ikke til å skille fra en
        // sunn økt: 'connected', men svart kanvas.
        this.bus.emit('error', error);
      },
    });
  }

  async sendAction(action: RemoteAction): Promise<ActionResult> {
    try {
      await this.dispatchAction(action);
      return { ok: true, action };
    } catch (error) {
      if (error instanceof ProtocolError || error instanceof TransportError) {
        return { ok: false, action, error };
      }
      throw error;
    }
  }

  private async dispatchAction(action: RemoteAction): Promise<void> {
    switch (action.type) {
      case 'pointer.move':
        await this.sendMouse(MouseEventKind.Move, 0, action.x, action.y);
        return;
      case 'pointer.click':
        await this.sendClick(action.button, action.x, action.y, 1);
        return;
      case 'pointer.doubleClick':
        await this.sendClick(action.button, action.x, action.y, 2);
        return;
      case 'pointer.down':
        await this.sendMouse(MouseEventKind.Down, buttonFlag(action.button), action.x, action.y);
        return;
      case 'pointer.up':
        await this.sendMouse(MouseEventKind.Up, buttonFlag(action.button), action.x, action.y);
        return;
      case 'pointer.scroll':
        // MERK: rulleenheter er IKKE definert av protokollen — verten
        // skalerer og snur fortegn plattformavhengig (×120 på Windows). Vi
        // sender deltaene som de er; skaleringen må finjusteres empirisk mot
        // en ekte vert. Se docs/rustdesk-protocol.md.
        await this.sendMouse(
          MouseEventKind.Wheel,
          0,
          Math.round(action.deltaX),
          Math.round(action.deltaY),
        );
        return;
      case 'keyboard.keyDown':
        await this.sendKey(action.key, true);
        return;
      case 'keyboard.keyUp':
        await this.sendKey(action.key, false);
        return;
      case 'keyboard.type':
        await this.sendEncrypted({
          kind: 'keyEvent',
          value: {
            down: true,
            press: true,
            identification: mapTextToIdentification(action.text),
            modifiers: [],
            mode: KeyboardMode.Legacy,
          },
        });
        return;
      case 'clipboard.write':
        await this.sendEncrypted({
          kind: 'clipboard',
          value: {
            compress: false,
            content: new TextEncoder().encode(action.text),
            width: 0,
            height: 0,
            format: ClipboardFormat.Text,
            specialName: '',
          },
        });
        return;
      case 'display.select': {
        // `RemoteDisplay.id` er en opak streng i den offentlige API-en. Vi slår
        // den derfor OPP i stedet for å tallkonvertere: `Number('')` er 0, så
        // en tom id ville ellers blitt et stilltiende, «vellykket» bytte til
        // skjerm 0. Verdien på ledningen er fortsatt indeksen (se
        // toRemoteDisplays).
        const index = this._displays.findIndex((display) => display.id === action.displayId);
        if (index < 0) {
          throw new ProtocolError(`display.select: unknown displayId "${action.displayId}"`, {
            displayId: action.displayId,
            displayCount: this._displays.length,
          });
        }
        // Vi flytter IKKE markøren her — verten bekrefter med sin egen
        // SwitchDisplay (med geometri), og først da oppdateres `displays`.
        await this.sendEncrypted({ kind: 'switchDisplay', display: index });
        return;
      }
      case 'quality.set': {
        // 'auto' har ingen egen enum-verdi: verten kjører sin ABR med mindre
        // vi pinner et nivå, så vi sender NotSet for å slippe styringen.
        const imageQuality = IMAGE_QUALITY_BY_LEVEL[action.level];
        // Kompatibilitetssjekken hos verten krever at en Misc.option med
        // supported_decoding sendes ALENE — derfor bare dette ene feltet her.
        await this.sendEncrypted({ kind: 'option', value: { imageQuality } });
        this.requestedQuality = action.level;
        this.bus.emit('quality', {
          level: action.level,
          targetBitrateKbps: this.lastReportedBitrateKbps,
        });
        return;
      }
      default: {
        const exhaustive: never = action;
        throw new ProtocolError(`Unhandled action: ${JSON.stringify(exhaustive)}`);
      }
    }
  }

  private sendMouse(kind: number, buttonFlags: number, x: number, y: number): Promise<void> {
    return this.sendEncrypted({
      kind: 'mouseEvent',
      value: { kind, buttonFlags, x: Math.round(x), y: Math.round(y), modifiers: [] },
    });
  }

  private async sendClick(
    button: 'left' | 'right' | 'middle',
    x: number,
    y: number,
    times: number,
  ): Promise<void> {
    const flag = buttonFlag(button);
    for (let index = 0; index < times; index += 1) {
      await this.sendMouse(MouseEventKind.Down, flag, x, y);
      await this.sendMouse(MouseEventKind.Up, flag, x, y);
    }
  }

  private sendKey(key: string, down: boolean): Promise<void> {
    return this.sendEncrypted({
      kind: 'keyEvent',
      value: {
        down,
        press: false,
        identification: mapBrowserKeyToIdentification(key),
        modifiers: [],
        mode: KeyboardMode.Legacy,
      },
    });
  }

  /**
   * Vertens sonde (`fromClient: false`) skal ekkoes uendret tilbake — det er
   * slik verten måler RTT og styrer bitrate; en klient som ikke ekkoer får
   * dårligere bilde. `lastDelay` i den er vertens forrige måling, en reell
   * RTT vi kan vise. Vår egen sonde (`fromClient: true`) kommer tilbake fra
   * verten uendret, og differansen mot `time` er vår måling.
   */
  private handleTestDelay(value: RdTestDelay): void {
    if (value.fromClient) {
      // proto3 utelater nullverdier, så «time mangler» og «time = 0» er samme
      // bytes. Uten denne sjekken ville en sonde uten tidsstempel blitt
      // rapportert som ~55 års forsinkelse — en oppdiktet måling.
      if (value.time <= 0n) {
        this.logger.debug('Ignoring a TestDelay echo with no timestamp');
        return;
      }
      const rtt = Date.now() - Number(value.time);
      if (rtt < 0 || rtt > MAX_PLAUSIBLE_LATENCY_MS) {
        this.logger.debug('Ignoring an implausible TestDelay round trip', { rtt });
        return;
      }
      this.bus.emit('latency', { latencyMs: rtt });
      return;
    }
    // Ekko er «best effort»: er kanalen nettopp borte, er det ingenting å
    // ekko til, og frakoblingen rapporteres uansett via 'disconnect'.
    this.sendEncrypted({ kind: 'testDelay', value }).catch((error: unknown) => {
      this.logger.debug('Could not echo TestDelay', {
        error: error instanceof Error ? error.message : String(error),
      });
    });
    if (value.lastDelay > 0) {
      this.bus.emit('latency', { latencyMs: value.lastDelay });
    }
    // Vertens eget måltall for enkoderen (kbps) — det ENESTE kvalitetstallet
    // protokollen faktisk rapporterer. Vi utleder ikke et nivå av det; vi
    // sender både nivået som gjelder og tallet, og bare når tallet endrer seg.
    if (value.targetBitrate > 0 && value.targetBitrate !== this.lastReportedBitrateKbps) {
      this.lastReportedBitrateKbps = value.targetBitrate;
      this.bus.emit('quality', {
        level: this.requestedQuality,
        targetBitrateKbps: value.targetBitrate,
      });
    }
  }

  /**
   * En full markørform. Alt her må feile MYKT: den innkommende kjeden lukker
   * innboksen ved kast, så en ugyldig markør fra verten skal koste oss én
   * markør, ikke hele økten.
   */
  private handleCursorData(value: RdCursorData): void {
    const { width, height } = value;
    if (
      !Number.isInteger(width) ||
      !Number.isInteger(height) ||
      width <= 0 ||
      height <= 0 ||
      width > MAX_CURSOR_DIMENSION ||
      height > MAX_CURSOR_DIMENSION
    ) {
      this.logger.warn('Ignoring a cursor with implausible dimensions', { width, height });
      return;
    }

    const expectedLength = width * height * 4;
    let rgba: Uint8Array;
    try {
      rgba = decompressExactly(value.colors, expectedLength);
    } catch (error) {
      this.logger.warn('Ignoring a cursor whose pixel data failed to decompress or had the wrong size', {
        expected: expectedLength,
        error: error instanceof Error ? error.message : String(error),
      });
      return;
    }

    const shape: RemoteCursorShape = {
      id: value.id.toString(),
      width,
      height,
      hotspotX: value.hotx,
      hotspotY: value.hoty,
      rgba,
    };
    // CursorData er alltid autoritativ for sin id — overskriv, ikke behold.
    this.cursorCache.delete(value.id);
    this.cursorCache.set(value.id, shape);
    while (this.cursorCache.size > MAX_CURSOR_CACHE) {
      const oldest = this.cursorCache.keys().next().value;
      if (oldest === undefined) break;
      this.cursorCache.delete(oldest);
    }
    this.bus.emit('cursor-shape', shape);
  }

  /** Verten gjenvelger en form den allerede har sendt. Ukjent id: behold nåværende markør, som referanseklienten. */
  private handleCursorId(id: bigint): void {
    const shape = this.cursorCache.get(id);
    if (!shape) {
      this.logger.debug('Host selected a cursor we never received; keeping the current one', { id: id.toString() });
      return;
    }
    // Berør oppføringen så «sist brukt» overlever cache-taket.
    this.cursorCache.delete(id);
    this.cursorCache.set(id, shape);
    this.bus.emit('cursor-shape', shape);
  }

  private handleCursorPosition(value: RdCursorPosition): void {
    const origin = this.displayOrigins[this.currentDisplayIndex] ?? { x: 0, y: 0 };
    this.bus.emit('cursor-position', {
      x: value.x - origin.x,
      y: value.y - origin.y,
      displayId: String(this.currentDisplayIndex),
    });
  }

  private startLatencyProbe(): void {
    const intervalMs = this.options.latencyProbeIntervalMs ?? 2_000;
    if (intervalMs <= 0) return;
    this.stopLatencyProbe();
    this.latencyProbe = setInterval(() => {
      if (this.state !== 'connected') return;
      this.sendEncrypted({
        kind: 'testDelay',
        value: { time: BigInt(Date.now()), fromClient: true, lastDelay: 0, targetBitrate: 0 },
      }).catch((error: unknown) => {
        this.logger.debug('Latency probe failed', {
          error: error instanceof Error ? error.message : String(error),
        });
      });
    }, intervalMs);
  }

  private stopLatencyProbe(): void {
    if (this.latencyProbe !== undefined) clearInterval(this.latencyProbe);
    this.latencyProbe = undefined;
  }

  /** Returnerer en frame KALLEREN eier og må frigjøre (se media/VideoFrame.ts). */
  async captureFrame(options?: CaptureFrameOptions): Promise<RemoteVideoFrame> {
    const retained = this.retainedFrame;
    if (!retained) {
      throw new CodecError('No video frame has been decoded yet');
    }
    return options?.region ? cropVideoFrame(retained, options.region) : cloneVideoFrame(retained);
  }

  async disconnect(): Promise<void> {
    if (this.state === 'disconnected' || this.state === 'failed') return;

    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;

    this.inbox.close(new TransportError('Protocol disconnected'));
    this.stopLatencyProbe();
    this.decoder?.close();
    this.decoder = undefined;

    if (this.retainedFrame) {
      releaseVideoFrame(this.retainedFrame);
      this.retainedFrame = undefined;
    }

    await this.relayTransport?.close().catch(() => undefined);
    this.relayTransport = undefined;
    this.secureChannel = undefined;
    this.transition('disconnected');
  }

  private transition(to: InternalSessionState): void {
    const from = this.state;
    this.state = to;
    this.bus.emit('state', { from, to });
  }
}

/** RustDesk kaller midtknappen "Wheel" i flaggsettet sitt. */
function buttonFlag(button: 'left' | 'right' | 'middle'): number {
  switch (button) {
    case 'left':
      return MouseButtonFlag.Left;
    case 'right':
      return MouseButtonFlag.Right;
    case 'middle':
      return MouseButtonFlag.Wheel;
  }
}

function randomSessionId(): bigint {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  return value;
}

/** RustDesk sender et lite, fast sett feilstrenger — oversett de vi kjenner. */
function describeLoginError(error: string): string {
  switch (error) {
    case 'Wrong Password':
      return 'The remote device rejected the password or session token';
    case 'Empty Password':
      return 'The remote device requires a password and none was supplied';
    case LOGIN_ERROR_2FA_REQUIRED:
      return 'The remote device requires two-factor authentication';
    case 'Wrong 2FA Code':
      return 'The two-factor code was rejected';
    case 'No Password Access':
      return 'The remote device does not allow password-based access';
    case 'Offline':
      return 'The remote device reported itself offline';
    default:
      return `The remote device refused the login: ${error}`;
  }
}
