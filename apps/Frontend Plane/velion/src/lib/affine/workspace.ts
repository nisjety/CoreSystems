/**
 * AFFiNE / BlockSuite workspace initialisation.
 *
 * Creates (or re-uses) a DocCollection backed by:
 *   - @blocksuite/sync IndexedDBDocSource for immediate local persistence
 *   - the planner's own snapshot persistence path, which hydrates and saves
 *     document state through the app's Convex-backed API routes
 *
 * NOTE: All BlockSuite imports are dynamic-only. LitElement custom-element
 * registrations must NEVER run on the server.
 */

import type { DocCollection } from '@blocksuite/store';

/** Singleton collection cache — one per workspace ID */
const workspaceHandles = new Map<string, WorkspaceHandle>();
const pendingDocs = new WeakMap<DocCollection, Map<string, Promise<ReturnType<DocCollection['createDoc']>>>>();

export interface WorkspaceHandle {
  collection: DocCollection;
  /** Disconnect sync providers and evict from cache */
  dispose: () => void;
}

// ─────────────────────────────────────────────────────────────────────────────
// Public: getOrCreateWorkspace
// ─────────────────────────────────────────────────────────────────────────────

export async function getOrCreateWorkspace(
  workspaceId: string
): Promise<WorkspaceHandle> {
  const existing = workspaceHandles.get(workspaceId);
  if (existing) return existing;

  // Dynamic imports — must not execute on the server
  const [
    { Schema, DocCollection },
    { AffineSchemas },
    { IndexedDBDocSource },
    { IndexedDBBlobSource },
  ] = await Promise.all([
    import('@blocksuite/store'),
    import('@blocksuite/blocks'),
    // @blocksuite/sync provides the native IndexedDB doc persistence source
    import('@blocksuite/sync') as Promise<{ IndexedDBDocSource: typeof import('@blocksuite/sync')['IndexedDBDocSource'] }>,
    import('@blocksuite/sync') as Promise<{ IndexedDBBlobSource: typeof import('@blocksuite/sync')['IndexedDBBlobSource'] }>,
  ]);

  const schema = new Schema().register(AffineSchemas);
  const dbName = `affine-${workspaceId}`;

  const collection = new DocCollection({
    id: workspaceId,
    schema,
    // IndexedDB keeps documents alive across page reloads (local-first)
    docSources: {
      main: new IndexedDBDocSource(dbName),
    },
    blobSources: {
      main: new IndexedDBBlobSource(dbName),
    },
  });

  collection.meta.initialize();

  const handle: WorkspaceHandle = {
    collection,
    dispose: () => {
      workspaceHandles.delete(workspaceId);
    },
  };

  workspaceHandles.set(workspaceId, handle);

  return handle;
}

// ─────────────────────────────────────────────────────────────────────────────
// Public: getOrCreateDoc
// ─────────────────────────────────────────────────────────────────────────────

export async function getOrCreateDoc(
  collection: DocCollection,
  docId: string
): Promise<ReturnType<DocCollection['createDoc']>> {
  const existing = collection.getDoc(docId);
  if (existing) {
    existing.load();
    return existing;
  }

  let collectionPendingDocs = pendingDocs.get(collection);
  if (!collectionPendingDocs) {
    collectionPendingDocs = new Map();
    pendingDocs.set(collection, collectionPendingDocs);
  }

  const pending = collectionPendingDocs.get(docId);
  if (pending) {
    return pending;
  }

  const created = (async () => {
    const alreadyCreated = collection.getDoc(docId);
    if (alreadyCreated) {
      alreadyCreated.load();
      return alreadyCreated;
    }

    const { Text } = await import('@blocksuite/store');

    const doc = collection.createDoc({ id: docId });

    doc.load(() => {
      const pageId = doc.addBlock('affine:page', { title: new Text('Planner') });
      // affine:surface is required for edgeless/canvas mode.
      // Cast through unknown because the TS overload union doesn't include all
      // flavour strings – they're registered at runtime via AffineSchemas.
      (doc.addBlock as (f: string, p: object, parent: string) => string)(
        'affine:surface', {}, pageId
      );
      const noteId = doc.addBlock('affine:note', {}, pageId);
      doc.addBlock('affine:paragraph', {}, noteId);
    });

    return doc;
  })().finally(() => {
    collectionPendingDocs.delete(docId);
    if (collectionPendingDocs.size === 0) {
      pendingDocs.delete(collection);
    }
  });

  collectionPendingDocs.set(docId, created);
  return created;
}

