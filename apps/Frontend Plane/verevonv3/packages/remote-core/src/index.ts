// ---------- Offentlig API ----------
export type {
  ActionMiddleware,
  ActionResult,
  ActionsController,
  AIController,
  AIObserver,
  AIObserverEventMap,
  AIObserverOptions,
  AuthenticationChallenge,
  AuthenticationResponse,
  CaptureFrameOptions,
  CaptureRegion,
  ClipboardController,
  ConnectOptions,
  DisconnectReason,
  KeyboardController,
  PermissionController,
  PointerButton,
  PointerController,
  QualityLevel,
  RemoteAction,
  RemoteActionType,
  RemoteClient,
  RemoteCursorShape,
  RemoteDisplay,
  RemoteEvent,
  RemoteEventHandler,
  RemoteEventMap,
  RemotePermission,
  RemoteSession,
  RemoteVideoFrame,
  SessionActor,
  SessionAuthenticator,
  SessionState,
  SessionStats,
  StatsController,
  Unsubscribe,
} from './types/public.js';

export {
  AuthenticationError,
  CodecError,
  EncryptionError,
  PermissionDeniedError,
  ProtocolError,
  RemoteConnectionError,
  RemoteDisconnectedError,
  RemoteError,
  TransportError,
} from './errors/RemoteError.js';
export type { RemoteErrorCode, RemoteErrorMetadata } from './errors/RemoteError.js';

export { createRemoteClient } from './client/createRemoteClient.js';
export type { CreateRemoteClientOptions } from './client/createRemoteClient.js';
export { DEFAULT_RECONNECT_POLICY } from './client/Reconnector.js';
export type { ReconnectPolicy } from './client/Reconnector.js';

export type { RemoteProtocol, ProtocolConnectOptions } from './protocol/RemoteProtocol.js';

export { CanvasRenderer } from './renderer/CanvasRenderer.js';
export { CoordinateMapper } from './renderer/CoordinateMapper.js';
export type { Point, Size } from './renderer/CoordinateMapper.js';

export { WebSocketTransport } from './transport/WebSocketTransport.js';
export type { Transport, TransportCloseInfo } from './transport/Transport.js';
export type { WebSocketFactory, WebSocketLike, WebSocketTransportOptions } from './transport/WebSocketTransport.js';

export { cloneVideoFrame, cropVideoFrame, releaseVideoFrame } from './media/VideoFrame.js';
export { detectMediaCapabilities } from './media/RemoteVideoDecoder.js';
export type { MediaCapabilities } from './media/RemoteVideoDecoder.js';

export type { Logger } from './logging/Logger.js';
export { noopLogger } from './logging/Logger.js';

// ---------- RustDesk-spesifikke byggeklosser ----------
// `createRemoteClient({ rendezvousUrl })` bygger denne protokollen selv, så
// vanlig bruk trenger ingenting herfra. Eksponert for kallere som vil sette
// opp protokollen eksplisitt (egne timeouts, egen transport, egen
// autentikator) — se docs/architecture.md, "RustDesk protocol isolation".
export { createRustDeskProtocol } from './protocol/rustdesk/createRustDeskProtocol.js';
export type { RustDeskProtocolOptions } from './protocol/rustdesk/RustDeskProtocol.js';
export { RustDeskPasswordAuthenticator } from './protocol/rustdesk/RustDeskPasswordAuthenticator.js';
