import * as Y from 'yjs';
import * as awarenessProtocol from 'y-protocols/awareness';
import * as syncProtocol from 'y-protocols/sync';
import * as decoding from 'lib0/decoding';
import * as encoding from 'lib0/encoding';
import * as eventloop from 'lib0/eventloop';
import * as map from 'lib0/map';
import { callbackHandler, isCallbackSet } from './callback.js';

const CALLBACK_DEBOUNCE_WAIT = parseInt(process.env.CALLBACK_DEBOUNCE_WAIT || '2000');
const CALLBACK_DEBOUNCE_MAXWAIT = parseInt(process.env.CALLBACK_DEBOUNCE_MAXWAIT || '10000');

const debouncer = eventloop.createDebouncer(
  CALLBACK_DEBOUNCE_WAIT,
  CALLBACK_DEBOUNCE_MAXWAIT,
);

const wsReadyStateConnecting = 0;
const wsReadyStateOpen = 1;

const gcEnabled = process.env.GC !== 'false' && process.env.GC !== '0';

let persistence = null;

export const setPersistence = (persistenceLayer) => {
  persistence = persistenceLayer;
};

export const getPersistence = () => persistence;

export const docs = new Map();

const messageSync = 0;
const messageAwareness = 1;

const updateHandler = (update, _origin, doc) => {
  const encoder = encoding.createEncoder();
  encoding.writeVarUint(encoder, messageSync);
  syncProtocol.writeUpdate(encoder, update);
  const message = encoding.toUint8Array(encoder);
  doc.conns.forEach((_value, conn) => send(doc, conn, message));
};

let contentInitializor = (_ydoc) => Promise.resolve();

export const setContentInitializor = (initializor) => {
  contentInitializor = initializor;
};

export class WSSharedDoc extends Y.Doc {
  constructor(name) {
    super({ gc: gcEnabled });

    this.name = name;
    this.conns = new Map();
    this.awareness = new awarenessProtocol.Awareness(this);
    this.awareness.setLocalState(null);

    const awarenessChangeHandler = ({ added, updated, removed }, conn) => {
      const changedClients = added.concat(updated, removed);

      if (conn !== null) {
        const connControlledIds = this.conns.get(conn);
        if (connControlledIds !== undefined) {
          added.forEach((clientId) => connControlledIds.add(clientId));
          removed.forEach((clientId) => connControlledIds.delete(clientId));
        }
      }

      const encoder = encoding.createEncoder();
      encoding.writeVarUint(encoder, messageAwareness);
      encoding.writeVarUint8Array(
        encoder,
        awarenessProtocol.encodeAwarenessUpdate(this.awareness, changedClients),
      );
      const buff = encoding.toUint8Array(encoder);

      this.conns.forEach((_value, connection) => {
        send(this, connection, buff);
      });
    };

    this.awareness.on('update', awarenessChangeHandler);
    this.on('update', updateHandler);

    if (isCallbackSet) {
      this.on('update', (_update, _origin, doc) => {
        debouncer(() => callbackHandler(doc));
      });
    }

    this.whenInitialized = contentInitializor(this);
  }
}

export const getYDoc = (docName, gc = true) => map.setIfUndefined(docs, docName, () => {
  const doc = new WSSharedDoc(docName);
  doc.gc = gc;

  if (persistence !== null) {
    persistence.bindState(docName, doc);
  }

  docs.set(docName, doc);
  return doc;
});

const messageListener = (conn, doc, message) => {
  try {
    const encoder = encoding.createEncoder();
    const decoder = decoding.createDecoder(message);
    const messageType = decoding.readVarUint(decoder);

    switch (messageType) {
      case messageSync:
        encoding.writeVarUint(encoder, messageSync);
        syncProtocol.readSyncMessage(decoder, encoder, doc, conn);

        if (encoding.length(encoder) > 1) {
          send(doc, conn, encoding.toUint8Array(encoder));
        }
        break;
      case messageAwareness:
        awarenessProtocol.applyAwarenessUpdate(
          doc.awareness,
          decoding.readVarUint8Array(decoder),
          conn,
        );
        break;
      default:
        break;
    }
  } catch (error) {
    console.error(error);
    doc.emit('error', [error]);
  }
};

const closeConn = (doc, conn) => {
  if (doc.conns.has(conn)) {
    const controlledIds = doc.conns.get(conn);
    doc.conns.delete(conn);

    awarenessProtocol.removeAwarenessStates(
      doc.awareness,
      Array.from(controlledIds),
      null,
    );

    if (doc.conns.size === 0 && persistence !== null) {
      persistence.writeState(doc.name, doc).then(() => {
        doc.destroy();
      });
      docs.delete(doc.name);
    }
  }

  conn.close();
};

const send = (doc, conn, message) => {
  if (conn.readyState !== wsReadyStateConnecting && conn.readyState !== wsReadyStateOpen) {
    closeConn(doc, conn);
    return;
  }

  try {
    conn.send(message, {}, (error) => {
      if (error != null) {
        closeConn(doc, conn);
      }
    });
  } catch {
    closeConn(doc, conn);
  }
};

const pingTimeout = 30_000;

export const setupWSConnection = (
  conn,
  req,
  { docName = (req.url || '').slice(1).split('?')[0], gc = true } = {},
) => {
  conn.binaryType = 'arraybuffer';

  const doc = getYDoc(docName, gc);
  doc.conns.set(conn, new Set());

  conn.on('message', (message) => {
    messageListener(conn, doc, new Uint8Array(message));
  });

  let pongReceived = true;

  const pingInterval = setInterval(() => {
    if (!pongReceived) {
      if (doc.conns.has(conn)) {
        closeConn(doc, conn);
      }
      clearInterval(pingInterval);
      return;
    }

    if (doc.conns.has(conn)) {
      pongReceived = false;
      try {
        conn.ping();
      } catch {
        closeConn(doc, conn);
        clearInterval(pingInterval);
      }
    }
  }, pingTimeout);

  conn.on('close', () => {
    closeConn(doc, conn);
    clearInterval(pingInterval);
  });

  conn.on('pong', () => {
    pongReceived = true;
  });

  const syncEncoder = encoding.createEncoder();
  encoding.writeVarUint(syncEncoder, messageSync);
  syncProtocol.writeSyncStep1(syncEncoder, doc);
  send(doc, conn, encoding.toUint8Array(syncEncoder));

  const awarenessStates = doc.awareness.getStates();
  if (awarenessStates.size > 0) {
    const awarenessEncoder = encoding.createEncoder();
    encoding.writeVarUint(awarenessEncoder, messageAwareness);
    encoding.writeVarUint8Array(
      awarenessEncoder,
      awarenessProtocol.encodeAwarenessUpdate(
        doc.awareness,
        Array.from(awarenessStates.keys()),
      ),
    );
    send(doc, conn, encoding.toUint8Array(awarenessEncoder));
  }
};