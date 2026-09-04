import type {
  ActionResult,
  CaptureFrameOptions,
  RemoteAction,
  RemoteDisplay,
  RemotePermission,
  RemoteVideoFrame,
  Unsubscribe,
} from '../../src/types/public.js';
import type { ProtocolConnectOptions, RemoteProtocol } from '../../src/protocol/RemoteProtocol.js';
import type { ProtocolEvent, ProtocolEventMap } from '../../src/types/internal.js';
import { EventBus } from '../../src/events/EventBus.js';

/**
 * Testdobbel for RemoteProtocol. Lar tester drive en økt (frame-, tilstands-
 * og frakoblingshendelser) uten en ekte RustDesk-server eller fjernmaskin —
 * se "The core must be testable without a real remote machine" i spec-en.
 */
export class MockProtocol implements RemoteProtocol {
  displays: readonly RemoteDisplay[] = [];
  permissions: readonly RemotePermission[] = [];
  readonly connectCalls: ProtocolConnectOptions[] = [];
  readonly sentActions: RemoteAction[] = [];
  disconnectCalls = 0;

  private readonly bus = new EventBus<ProtocolEventMap>();
  private nextActionResult: ActionResult | undefined;
  private connectImpl: (options: ProtocolConnectOptions) => Promise<void> = async () => undefined;

  setConnectBehavior(impl: (options: ProtocolConnectOptions) => Promise<void>): void {
    this.connectImpl = impl;
  }

  setNextActionResult(result: ActionResult): void {
    this.nextActionResult = result;
  }

  async connect(options: ProtocolConnectOptions): Promise<void> {
    this.connectCalls.push(options);
    await this.connectImpl(options);
  }

  async sendAction(action: RemoteAction): Promise<ActionResult> {
    this.sentActions.push(action);
    const result = this.nextActionResult ?? { ok: true, action };
    this.nextActionResult = undefined;
    return result;
  }

  async captureFrame(_options?: CaptureFrameOptions): Promise<RemoteVideoFrame> {
    return { width: 0, height: 0, timestamp: 0 };
  }

  async disconnect(): Promise<void> {
    this.disconnectCalls += 1;
  }

  on<T extends ProtocolEvent>(event: T, handler: (payload: ProtocolEventMap[T]) => void): Unsubscribe {
    return this.bus.on(event, handler);
  }

  /** Testhjelper: sender en hendelse som om den kom fra ekte protokoll-I/O. */
  emit<T extends ProtocolEvent>(event: T, payload: ProtocolEventMap[T]): void {
    this.bus.emit(event, payload);
  }
}
