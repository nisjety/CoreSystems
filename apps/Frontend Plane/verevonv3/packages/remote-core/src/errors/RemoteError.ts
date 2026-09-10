export type RemoteErrorCode =
  | 'CONNECTION_FAILED'
  | 'AUTHENTICATION_FAILED'
  | 'PERMISSION_DENIED'
  | 'PROTOCOL_ERROR'
  | 'TRANSPORT_ERROR'
  | 'CODEC_ERROR'
  | 'ENCRYPTION_ERROR'
  | 'REMOTE_DISCONNECTED';

export interface RemoteErrorMetadata {
  readonly [key: string]: string | number | boolean | undefined;
}

/**
 * Basisklasse for alle feil som eksponeres til Verevon-UI. `metadata` skal
 * ALDRI inneholde passord, nøkler, tokens eller rå protokoll-payloads — kun
 * trygg, diagnostisk kontekst (f.eks. url, kode, forsøksnummer).
 */
export class RemoteError extends Error {
  readonly code: RemoteErrorCode;
  readonly metadata: RemoteErrorMetadata;

  constructor(code: RemoteErrorCode, message: string, metadata: RemoteErrorMetadata = {}) {
    super(message);
    this.name = 'RemoteError';
    this.code = code;
    this.metadata = Object.freeze({ ...metadata });
  }
}

export class RemoteConnectionError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('CONNECTION_FAILED', message, metadata);
    this.name = 'RemoteConnectionError';
  }
}

export class AuthenticationError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('AUTHENTICATION_FAILED', message, metadata);
    this.name = 'AuthenticationError';
  }
}

export class PermissionDeniedError extends RemoteError {
  readonly permission: string;

  constructor(permission: string, metadata?: RemoteErrorMetadata) {
    super('PERMISSION_DENIED', `Permission denied: ${permission}`, metadata);
    this.name = 'PermissionDeniedError';
    this.permission = permission;
  }
}

export class ProtocolError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('PROTOCOL_ERROR', message, metadata);
    this.name = 'ProtocolError';
  }
}

export class TransportError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('TRANSPORT_ERROR', message, metadata);
    this.name = 'TransportError';
  }
}

export class CodecError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('CODEC_ERROR', message, metadata);
    this.name = 'CodecError';
  }
}

export class EncryptionError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('ENCRYPTION_ERROR', message, metadata);
    this.name = 'EncryptionError';
  }
}

export class RemoteDisconnectedError extends RemoteError {
  constructor(message: string, metadata?: RemoteErrorMetadata) {
    super('REMOTE_DISCONNECTED', message, metadata);
    this.name = 'RemoteDisconnectedError';
  }
}
