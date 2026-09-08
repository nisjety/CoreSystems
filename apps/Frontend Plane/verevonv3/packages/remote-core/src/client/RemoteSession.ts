import type {
  ActionsController,
  AIController,
  AIObserverOptions,
  CaptureFrameOptions,
  ClipboardController,
  DisconnectReason,
  KeyboardController,
  PermissionController,
  PointerController,
  RemoteDisplay,
  RemoteEvent,
  RemoteEventHandler,
  RemoteSession as IRemoteSession,
  RemoteVideoFrame,
  SessionState,
  StatsController,
  Unsubscribe,
} from '../types/public.js';
import type { InternalSessionState, RenderStatsSink } from '../types/internal.js';
import { classifyDisconnect, isRetryableDisconnectReason } from '../types/internal.js';
import type { RemoteProtocol } from '../protocol/RemoteProtocol.js';
import type { Logger } from '../logging/Logger.js';
import { noopLogger } from '../logging/Logger.js';
import { EventBus } from '../events/EventBus.js';
import { PermissionManager } from '../permissions/PermissionManager.js';
import { ActionExecutor } from '../actions/ActionExecutor.js';
import { SessionStateMachine } from './SessionStateMachine.js';
import { SessionStatsTracker } from './SessionStatsTracker.js';
import { DEFAULT_RECONNECT_POLICY, Reconnector, type ReconnectPolicy } from './Reconnector.js';
import { RemoteConnectionError } from '../errors/RemoteError.js';
import { FrameSampler } from '../ai/FrameSampler.js';
import { AIObserverImpl } from '../ai/AIObserver.js';
import { cloneVideoFrame, releaseVideoFrame } from '../media/VideoFrame.js';
import { PointerControllerImpl } from '../input/PointerController.js';
import { KeyboardControllerImpl } from '../input/KeyboardController.js';
import { ClipboardControllerImpl } from '../clipboard/ClipboardController.js';

export function toPublicState(state: InternalSessionState): SessionState {
  switch (state) {
    case 'idle':
    case 'connecting':
    case 'rendezvous':
      // 'rendezvous' er en RustDesk-spesifikk underfase av tilkobling —
      // UI-laget trenger ikke skille den fra resten av "connecting".
      return 'connecting';
    case 'authenticating':
      return 'authenticating';
    case 'connected':
      return 'connected';
    case 'reconnecting':
      return 'reconnecting';
    case 'disconnected':
      return 'disconnected';
    case 'failed':
      return 'failed';
    default: {
      const exhaustive: never = state;
      throw new Error(`Unhandled internal session state: ${String(exhaustive)}`);
    }
  }
}

export interface RemoteSessionImplOptions {
  readonly id: string;
  readonly protocol: RemoteProtocol;
  readonly stateMachine: SessionStateMachine;
  readonly logger?: Logger;
  /**
   * Gjør ETT fullt nytt tilkoblingsforsøk med samme legitimasjon (leveres av
   * RemoteClient, som er den eneste som kjenner den). Uten denne kan et
   * forbigående tap ikke gjenopprettes, og økten lander direkte i
   * 'disconnected' i stedet for å late som den prøver.
   */
  readonly reconnect?: () => Promise<void>;
  readonly reconnectPolicy?: ReconnectPolicy;
  /** Kun for tester: deterministisk backoff. */
  readonly sleep?: (ms: number) => Promise<void>;
}

/**
 * Konkret implementasjon av den offentlige RemoteSession-typen. Eier all
 * øktstatus (tilstandsmaskin, tillatelser, statistikk, aktive AI-observatører)
 * og oversetter RemoteProtocol sine interne hendelser til den offentlige,
 * RustDesk-uavhengige hendelsesoverflaten.
 */
export class RemoteSessionImpl implements IRemoteSession, RenderStatsSink {
  readonly id: string;
  readonly pointer: PointerController;
  readonly keyboard: KeyboardController;
  readonly clipboard: ClipboardController;
  readonly permissions: PermissionController;
  readonly actions: ActionsController;
  readonly ai: AIController;
  readonly stats: StatsController;

  private readonly protocol: RemoteProtocol;
  private readonly stateMachine: SessionStateMachine;
  private readonly logger: Logger;
  private readonly bus = new EventBus<import('../types/public.js').RemoteEventMap>();
  private readonly permissionManager: PermissionManager;
  private readonly actionExecutor: ActionExecutor;
  private readonly clipboardImpl: ClipboardControllerImpl;
  private readonly statsTracker = new SessionStatsTracker();
  private readonly activeSamplers = new Set<FrameSampler>();
  private readonly unsubscribers: Unsubscribe[] = [];
  private readonly reconnect: (() => Promise<void>) | undefined;
  private readonly reconnectPolicy: ReconnectPolicy;
  private readonly sleep: ((ms: number) => Promise<void>) | undefined;
  private reconnector: Reconnector | undefined;
  private _displays: readonly RemoteDisplay[];
  private disposed = false;

  constructor(options: RemoteSessionImplOptions) {
    this.id = options.id;
    this.protocol = options.protocol;
    this.stateMachine = options.stateMachine;
    this.logger = options.logger ?? noopLogger;
    this.reconnect = options.reconnect;
    this.reconnectPolicy = options.reconnectPolicy ?? DEFAULT_RECONNECT_POLICY;
    this.sleep = options.sleep;
    this._displays = options.protocol.displays;

    this.permissionManager = new PermissionManager(options.protocol.permissions);
    this.actionExecutor = new ActionExecutor(this.protocol, this.permissionManager, this.logger);
    this.permissions = this.permissionManager;
    this.pointer = new PointerControllerImpl(this.actionExecutor, 'human');
    this.keyboard = new KeyboardControllerImpl(this.actionExecutor, 'human');
    this.clipboardImpl = new ClipboardControllerImpl(this.actionExecutor, 'human');
    this.clipboard = this.clipboardImpl;

    this.actions = {
      execute: (action) => this.actionExecutor.execute(action),
      use: (middleware) => this.actionExecutor.use(middleware),
    };
    this.ai = {
      createObserver: (observerOptions: AIObserverOptions) => this.createObserver(observerOptions),
    };
    this.stats = {
      getSnapshot: () => this.statsTracker.getSnapshot(),
    };

    this.wireProtocolEvents();
  }

  get state(): SessionState {
    return toPublicState(this.stateMachine.state);
  }

  get displays(): readonly RemoteDisplay[] {
    return this._displays;
  }

  /**
   * Meldes inn av rendereren for hver frame den faktisk maler (se
   * RenderStatsSink). Ligger bevisst ikke på den offentlige RemoteSession-
   * typen — kallere skal lese `stats`, ikke skrive til den.
   */
  recordRenderedFrame(): void {
    this.statsTracker.recordRenderedFrame();
  }

  captureFrame(options?: CaptureFrameOptions): Promise<RemoteVideoFrame> {
    return this.protocol.captureFrame(options);
  }

  async disconnect(): Promise<void> {
    if (this.disposed) return;
    // Et brukerinitiert avbrudd midt i en gjenoppkobling skal stoppe neste
    // forsøk — ikke la backoff-løkken koble opp igjen bak brukerens rygg.
    this.reconnector?.cancel();
    await this.protocol.disconnect();
    if (this.stateMachine.canTransition('disconnected')) {
      const previous = this.state;
      this.stateMachine.transition('disconnected', 'user');
      this.bus.emit('state', { state: 'disconnected', previous });
    }
    this.teardown();
  }

  on<T extends RemoteEvent>(event: T, handler: RemoteEventHandler<T>): Unsubscribe {
    return this.bus.on(event, handler);
  }

  private createObserver(options: AIObserverOptions) {
    const sampler = new FrameSampler({
      maxFramesPerSecond: options.maxFramesPerSecond,
      displayId: options.displayId,
    });
    this.activeSamplers.add(sampler);
    return new AIObserverImpl(sampler, () => this.activeSamplers.delete(sampler));
  }

  private wireProtocolEvents(): void {
    this.unsubscribers.push(
      this.protocol.on('frame', (frame) => {
        this.statsTracker.recordDecodedFrame();

        // Økten eier framen gjennom hele utsendelsen. Synkrone forbrukere
        // (CanvasRenderer) leser den direkte; AI-observatører får sin EGEN
        // kopi fordi de typisk bruker den asynkront, etter at vi har
        // frigjort originalen. Se eierskapsreglene i media/VideoFrame.ts.
        try {
          this.bus.emit('frame', frame);
          for (const sampler of this.activeSamplers) {
            sampler.submit(cloneVideoFrame(frame));
          }
        } finally {
          releaseVideoFrame(frame);
        }
      }),
    );

    this.unsubscribers.push(
      this.protocol.on('display-change', (payload) => {
        this._displays = payload.displays;
        this.bus.emit('display-change', payload);
      }),
    );

    this.unsubscribers.push(
      this.protocol.on('latency', (payload) => {
        this.statsTracker.recordLatency(payload.latencyMs);
        this.bus.emit('latency', payload);
      }),
    );

    this.unsubscribers.push(this.protocol.on('quality', (payload) => this.bus.emit('quality', payload)));
    this.unsubscribers.push(this.protocol.on('cursor-shape', (shape) => this.bus.emit('cursor-shape', shape)));
    this.unsubscribers.push(
      this.protocol.on('cursor-position', (position) => this.bus.emit('cursor-position', position)),
    );

    this.unsubscribers.push(
      this.protocol.on('media-stats', ({ encodedBytes, droppedFramesTotal }) => {
        this.statsTracker.recordEncodedBytes(encodedBytes);
        this.statsTracker.setDroppedFrames(droppedFramesTotal);
      }),
    );

    // Ikke-fatale protokollfeil (typisk dekoderfeil) skal nå kalleren. De
    // avslutter ikke økten, men en videostrøm som har stanset for godt må
    // kunne skilles fra en sunn økt.
    this.unsubscribers.push(this.protocol.on('error', (error) => this.bus.emit('error', error)));

    this.unsubscribers.push(
      this.protocol.on('permission-change', (payload) => {
        this.permissionManager.update(payload.permissions);
        this.bus.emit('permission-change', payload);
      }),
    );

    this.unsubscribers.push(
      this.protocol.on('clipboard', (payload) => {
        this.clipboardImpl.handleRemoteUpdate(payload.text);
        this.bus.emit('clipboard', payload);
      }),
    );

    this.unsubscribers.push(
      this.protocol.on('state', (change) => {
        // Terminale tilstander kommer ALLTID via 'disconnect' (med årsak) —
        // speiles de her også, ville et mislykket gjenoppkoblingsforsøk
        // (protokollen lander i 'failed') avbryte backoff-løkken for tidlig.
        if (change.to === 'disconnected' || change.to === 'failed') return;
        if (!this.stateMachine.canTransition(change.to)) return;
        const previous = this.state;
        this.stateMachine.transition(change.to);
        this.bus.emit('state', { state: this.state, previous });
      }),
    );

    this.unsubscribers.push(
      this.protocol.on('disconnect', ({ reason }) => {
        // Under en gjenoppkobling er det Reconnector som eier utfallet for de
        // FORBIGÅENDE årsakene: et forsøk som feiler avvises der, og økten
        // skal ikke rives ned av det. En ikke-forbigående årsak (verten er
        // stengt ned, tillatelsen er trukket) betyr derimot at det ikke er
        // noe å komme tilbake til — da må vi stanse løkken og rapportere den
        // ekte årsaken, i stedet for å bruke opp alle forsøkene og til slutt
        // melde en oppdiktet «ga opp etter N forsøk».
        if (this.stateMachine.state === 'reconnecting') {
          if (isRetryableDisconnectReason(reason)) return;
          this.reconnector?.cancel();
          this.reconnector = undefined;
          const previousState = this.state;
          const target = classifyDisconnect(reason, false);
          if (this.stateMachine.canTransition(target)) {
            this.stateMachine.transition(target, reason);
            this.bus.emit('state', { state: this.state, previous: previousState });
          }
          this.bus.emit('disconnect', { reason });
          this.teardown();
          return;
        }

        const wasConnected = this.stateMachine.state === 'connected';
        let target = classifyDisconnect(reason, wasConnected);
        if (target === 'reconnecting' && !this.canReconnect()) {
          // Ingen mulighet til å prøve igjen — da er 'reconnecting' en løgn.
          target = 'disconnected';
        }
        if (this.stateMachine.canTransition(target)) {
          const previous = this.state;
          this.stateMachine.transition(target, reason);
          this.bus.emit('state', { state: this.state, previous });
        }
        this.bus.emit('disconnect', { reason });
        if (target === 'reconnecting') {
          void this.runReconnect(reason);
        } else {
          this.teardown();
        }
      }),
    );
  }

  private canReconnect(): boolean {
    return this.reconnect !== undefined && this.reconnectPolicy.maxAttempts > 0;
  }

  private async runReconnect(reason: DisconnectReason): Promise<void> {
    const reconnect = this.reconnect;
    if (!reconnect) return;

    const reconnector = new Reconnector({
      policy: this.reconnectPolicy,
      attempt: reconnect,
      logger: this.logger,
      sleep: this.sleep,
    });
    this.reconnector = reconnector;
    const outcome = await reconnector.run();
    this.reconnector = undefined;

    // Brukeren kan ha koblet fra i mellomtiden; da er økten allerede revet.
    if (this.disposed || this.stateMachine.state !== 'reconnecting') return;
    // Et avbrutt forsøk er ikke en feil — den som avbrøt eier sluttilstanden.
    if (outcome.kind === 'cancelled') return;

    if (outcome.kind === 'succeeded') {
      // Protokollen har normalt allerede meldt 'connected' via 'state' og
      // sendt nye display-/tillatelseshendelser; dette er sikkerhetsnettet.
      this._displays = this.protocol.displays;
      this.permissionManager.update(this.protocol.permissions);
      if (this.stateMachine.canTransition('connected')) {
        const previous = this.state;
        this.stateMachine.transition('connected');
        this.bus.emit('state', { state: this.state, previous });
      }
      return;
    }

    const error =
      outcome.kind === 'refused'
        ? new RemoteConnectionError(
            `Reconnect was refused and will not be retried: ${outcome.error.message}`,
            { reason },
          )
        : new RemoteConnectionError(
            `Reconnect gave up after ${outcome.attempts} attempt(s) (original reason: ${reason}` +
              `${outcome.lastError ? `, last error: ${outcome.lastError.message}` : ''})`,
            { reason, maxAttempts: this.reconnectPolicy.maxAttempts },
          );
    this.logger.error(error.message);
    this.bus.emit('error', error);
    const previous = this.state;
    // Guard selv om den dominerende sjekken over gjør dette utilgjengelig i
    // dag: en enkelt ny `await` mellom dem ville ellers gjort en ulovlig
    // overgang til et kast.
    if (this.stateMachine.canTransition('failed')) {
      this.stateMachine.transition('failed', reason);
      this.bus.emit('state', { state: this.state, previous });
    }
    this.teardown();
  }

  private teardown(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    for (const sampler of this.activeSamplers) sampler.close();
    this.activeSamplers.clear();
    this.bus.clear();
  }
}
