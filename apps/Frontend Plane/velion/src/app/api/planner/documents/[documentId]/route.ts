import { NextRequest, NextResponse } from 'next/server';
import {
  archivePlannerDocument,
  renamePlannerDocument,
  restorePlannerDocument,
  updatePlannerDocument,
} from '@/lib/convex/planner-server';

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ documentId: string }> }
) {
  try {
    const { documentId } = await params;
    const body = (await request.json()) as {
      workspaceId?: string;
      title?: string;
      parentDocumentId?: string | null;
      isFavorite?: boolean;
      lastViewedAt?: number;
      restore?: boolean;
      space?: 'private' | 'shared' | 'collection';
    };
    const workspaceId = body.workspaceId?.trim();
    const title = body.title?.trim();

    if (!workspaceId) {
      return NextResponse.json({ error: 'workspaceId is required' }, { status: 400 });
    }

    if (body.restore) {
      const document = await restorePlannerDocument({ workspaceId, documentId });
      return NextResponse.json({ document });
    }

    if (title && body.parentDocumentId === undefined && body.isFavorite === undefined && body.lastViewedAt === undefined && body.space === undefined) {
      const document = await renamePlannerDocument({
        workspaceId,
        documentId,
        title,
      });
      return NextResponse.json({ document });
    }

    if (
      title === undefined &&
      body.parentDocumentId === undefined &&
      body.isFavorite === undefined &&
      body.lastViewedAt === undefined &&
      body.space === undefined
    ) {
      return NextResponse.json({ error: 'No planner document updates were provided' }, { status: 400 });
    }

    const document = await updatePlannerDocument({
      workspaceId,
      documentId,
      title,
      parentDocumentId:
        body.parentDocumentId === undefined ? undefined : (body.parentDocumentId?.trim() ?? null),
      isFavorite: body.isFavorite,
      lastViewedAt: body.lastViewedAt,
      space: body.space,
    });

    return NextResponse.json({ document });
  } catch (error) {
    console.error('[planner/document] update failed', error);
    return NextResponse.json({ error: 'Failed to update planner document' }, { status: 500 });
  }
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ documentId: string }> }
) {
  try {
    const { documentId } = await params;
    const workspaceId = request.nextUrl.searchParams.get('workspaceId')?.trim();

    if (!workspaceId) {
      return NextResponse.json({ error: 'workspaceId is required' }, { status: 400 });
    }

    const document = await archivePlannerDocument({ workspaceId, documentId });
    return NextResponse.json({ document });
  } catch (error) {
    console.error('[planner/document] archive failed', error);
    return NextResponse.json({ error: 'Failed to archive planner document' }, { status: 500 });
  }
}