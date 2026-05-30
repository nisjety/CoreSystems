import 'server-only';
import { z } from 'zod';

import { resolveChatActor } from '@/app/api/chat/_lib/session-store';
import { convexMutation, convexQuery } from '@/app/api/_lib/convex-client';
import type { KnowledgeQnAEntry } from '@/components/knowledge/types';

const qaCreateSchema = z.object({
  question: z.string().trim().min(3).max(500),
  answer: z.string().trim().min(1).max(8000),
  status: z.enum(['draft', 'published']).optional().default('draft'),
});

const qaUpdateSchema = z.object({
  question: z.string().trim().min(3).max(500).optional(),
  answer: z.string().trim().min(1).max(8000).optional(),
  status: z.enum(['draft', 'published', 'deprecated']).optional(),
});

interface ConvexQnAEntry {
  _id: string;
  orgId: string;
  question: string;
  answer: string;
  status: 'draft' | 'published' | 'deprecated';
  rating?: 'good' | 'acceptable' | 'poor';
  citationCount?: number;
  createdAt: number;
  updatedAt: number;
}

function normalize(entry: ConvexQnAEntry): KnowledgeQnAEntry {
  return {
    id: entry._id,
    orgId: entry.orgId,
    question: entry.question,
    answer: entry.answer,
    status: entry.status,
    rating: entry.rating,
    citationCount: entry.citationCount ?? 0,
    createdAt: entry.createdAt,
    updatedAt: entry.updatedAt,
  };
}

/**
 * Wave 11 §2.1: Q&A is a first-class entity (not a doc subtype).
 * Persistence lives in Convex `knowledgeQnA` — a Convex schema addition
 * that lands together with this file. The retrieval engine picks up Q&A
 * via `wiki-store-go` once D4/D5 ship; until then the playground
 * answer-eval surface in Phase 5 surfaces them as citation pills the
 * operator can flag.
 */
export async function listQnAEntries(): Promise<KnowledgeQnAEntry[]> {
  try {
    const actor = await resolveChatActor();
    const entries = await convexQuery<ConvexQnAEntry[]>('knowledgeQnA:listByOrg', {
      orgId: actor.convexOrgId,
    });
    return entries.map(normalize);
  } catch {
    // Convex unreachable or knowledgeQnA module not yet deployed — UI
    // renders empty-state, not a broken page.
    return [];
  }
}

export async function createQnAEntry(
  body: unknown,
): Promise<KnowledgeQnAEntry> {
  const parsed = qaCreateSchema.parse(body);
  const actor = await resolveChatActor();
  const created = await convexMutation<ConvexQnAEntry>('knowledgeQnA:create', {
    orgId: actor.convexOrgId,
    createdBy: actor.convexUserId,
    question: parsed.question,
    answer: parsed.answer,
    status: parsed.status,
  });
  return normalize(created);
}

export async function updateQnAEntry(
  id: string,
  body: unknown,
): Promise<KnowledgeQnAEntry> {
  const parsed = qaUpdateSchema.parse(body);
  const actor = await resolveChatActor();
  const updated = await convexMutation<ConvexQnAEntry>('knowledgeQnA:update', {
    entryId: id,
    orgId: actor.convexOrgId,
    ...parsed,
  });
  return normalize(updated);
}

export async function deleteQnAEntry(id: string): Promise<void> {
  const actor = await resolveChatActor();
  await convexMutation('knowledgeQnA:remove', {
    entryId: id,
    orgId: actor.convexOrgId,
  });
}
