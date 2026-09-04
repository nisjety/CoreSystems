import type {
  ActionsController,
  AIController,
  AIObserverOptions,
  CaptureFrameOptions,
  ClipboardController,
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
import type { InternalSessionState } from '../types/internal.js';
import { classifyDisconnect } from '../types/internal.js';
import type { RemoteProtocol } from '../protocol/RemoteProtocol.js';
import type { Logger } from '../logging/Logger.js';
import { noopLogger } from '../logging/Logger.js';
import { EventBus } from '../events/EventBus.js';
import { PermissionManager } from '../permissions/PermissionManager.js';
import { ActionExecutor } from '../actions/ActionExecutor.js';
import { SessionStateMachine } from './SessionStateMachine.js';
import { SessionStatsTracker } from './SessionStatsTracker.js';
import { FrameSampler } from '../ai/FrameSampler.js';
import { AIObserverImpl } from '../ai/AIObserver.js';
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
}

/**
 * Konkret implementasjon av den offentlige RemoteSession-typen. Eier all
 * øktstatus (tilstandsmaskin, tillatelser, statistikk, aktive AI-observatører)
 * og oversetter RemoteProtocol sine interne hendelser til den offentlige,
 * RustDesk-uavhengige hendelsesoverflaten.
 */
export class RemoteSessionImpl implements IRemoteSession {
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
  private _displays: readonly RemoteDisplay[];
  private disposed = false;

  constructor(options: RemoteSessionImplOptions) {
    this.id = options.id;
    this.protocol = options.protocol;
    this.stateMachine = options.stateMachine;
    this.logger = options.logger ?? noopLogger;
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

  captureFrame(options?: CaptureFrameOptions): Promise<RemoteVideoFrame> {
    return this.protocol.captureFrame(options);
  }

  async disconnect(): Promise<void> {
    if (this.disposed) return;
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
        this.bus.emit('frame', frame);
        for (const sampler of this.activeSamplers) sampler.submit(frame);
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
        if (!this.stateMachine.canTransition(change.to)) return;
        const previous = this.state;
        this.stateMachine.transition(change.to);
        this.bus.emit('state', { state: this.state, previous });
      }),
    );

    this.unsubscribers.push(
      this.protocol.on('disconnect', ({ reason }) => {
        const wasConnected = this.stateMachine.state === 'connected';
        const target = classifyDisconnect(reason, wasConnected);
        if (this.stateMachine.canTransition(target)) {
          const previous = this.state;
          this.stateMachine.transition(target, reason);
          this.bus.emit('state', { state: this.state, previous });
        }
        this.bus.emit('disconnect', { reason });
        if (target !== 'reconnecting') {
          this.teardown();
        }
      }),
    );
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
