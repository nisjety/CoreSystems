import 'server-only';

import { ConvexHttpClient } from 'convex/browser';
import { makeFunctionReference } from 'convex/server';

export interface PlannerDocumentRecord {
  _id: string;
  _creationTime: number;
  workspaceId: string;
  documentId: string;
  title: string;
  ownerExternalAuthId?: string;
  parentDocumentId?: string;
  isFavorite?: boolean;
  lastViewedAt?: number;
  space?: 'private' | 'shared' | 'collection';
  createdAt: number;
  updatedAt: number;
  archivedAt?: number;
}

export interface PlannerDocumentStateRecord {
  _id: string;
  _creationTime: number;
  workspaceId: string;
  documentId: string;
  stateBase64: string;
  updatedAt: number;
}

const listByWorkspaceRef = makeFunctionReference<'query'>('plannerDocuments:listByWorkspace');
const createRef = makeFunctionReference<'mutation'>('plannerDocuments:create');
const updateRef = makeFunctionReference<'mutation'>('plannerDocuments:update');
const renameRef = makeFunctionReference<'mutation'>('plannerDocuments:rename');
const archiveRef = makeFunctionReference<'mutation'>('plannerDocuments:archive');
const restoreRef = makeFunctionReference<'mutation'>('plannerDocuments:restore');
const getStateRef = makeFunctionReference<'query'>('plannerDocuments:getState');
const saveStateRef = makeFunctionReference<'mutation'>('plannerDocuments:saveState');

function getConvexUrl() {
  return (
    process.env.CONVEX_SELF_HOSTED_URL ||
    process.env.NEXT_PUBLIC_CONVEX_URL ||
    'http://127.0.0.1:3210'
  );
}

function createClient() {
  return new ConvexHttpClient(getConvexUrl(), {
    skipConvexDeploymentUrlCheck: true,
    logger: false,
  });
}

export async function listPlannerDocuments(workspaceId: string, includeArchived = false) {
  const client = createClient();
  return client.query(listByWorkspaceRef, {
    workspaceId,
    includeArchived,
  }) as Promise<PlannerDocumentRecord[]>;
}

export async function createPlannerDocument(input: {
  workspaceId: string;
  documentId: string;
  title: string;
  ownerExternalAuthId?: string;
  parentDocumentId?: string | null;
  space?: 'private' | 'shared' | 'collection';
}) {
  const client = createClient();
  return client.mutation(createRef, input) as Promise<PlannerDocumentRecord>;
}

export async function updatePlannerDocument(input: {
  workspaceId: string;
  documentId: string;
  title?: string;
  parentDocumentId?: string | null;
  isFavorite?: boolean;
  lastViewedAt?: number;
  space?: 'private' | 'shared' | 'collection';
}) {
  const client = createClient();
  return client.mutation(updateRef, input) as Promise<PlannerDocumentRecord>;
}

export async function renamePlannerDocument(input: {
  workspaceId: string;
  documentId: string;
  title: string;
}) {
  const client = createClient();
  return client.mutation(renameRef, input) as Promise<PlannerDocumentRecord>;
}

export async function archivePlannerDocument(input: {
  workspaceId: string;
  documentId: string;
}) {
  const client = createClient();
  return client.mutation(archiveRef, input) as Promise<PlannerDocumentRecord | null>;
}

export async function restorePlannerDocument(input: {
  workspaceId: string;
  documentId: string;
}) {
  const client = createClient();
  return client.mutation(restoreRef, input) as Promise<PlannerDocumentRecord | null>;
}

export async function getPlannerDocumentState(input: {
  workspaceId: string;
  documentId: string;
}) {
  const client = createClient();
  return client.query(getStateRef, input) as Promise<PlannerDocumentStateRecord | null>;
}

export async function savePlannerDocumentState(input: {
  workspaceId: string;
  documentId: string;
  stateBase64: string;
}) {
  const client = createClient();
  return client.mutation(saveStateRef, input) as Promise<{ updatedAt: number }>;
}