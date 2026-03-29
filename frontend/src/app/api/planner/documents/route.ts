import { NextRequest, NextResponse } from 'next/server';
import { getServerSession } from '@/components/auth/lib/auth-server';
import {
  createPlannerDocument,
  listPlannerDocuments,
} from '@/lib/convex/planner-server';

export async function GET(request: NextRequest) {
  const workspaceId = request.nextUrl.searchParams.get('workspaceId')?.trim();
  const includeArchived = request.nextUrl.searchParams.get('includeArchived') === 'true';

  if (!workspaceId) {
    return NextResponse.json({ error: 'workspaceId is required' }, { status: 400 });
  }

  try {
    const documents = await listPlannerDocuments(workspaceId, includeArchived);
    return NextResponse.json({ documents });
  } catch (error) {
    console.error('[planner/documents] list failed', error);
    return NextResponse.json({ error: 'Failed to load planner documents' }, { status: 500 });
  }
}

export async function POST(request: NextRequest) {
  try {
    const body = (await request.json()) as {
      workspaceId?: string;
      title?: string;
      parentDocumentId?: string | null;
      space?: 'private' | 'shared' | 'collection';
    };
    const workspaceId = body.workspaceId?.trim();

    if (!workspaceId) {
      return NextResponse.json({ error: 'workspaceId is required' }, { status: 400 });
    }

    const session = await getServerSession();
    const document = await createPlannerDocument({
      workspaceId,
      documentId: crypto.randomUUID(),
      title: body.title ?? 'Untitled note',
      ownerExternalAuthId: session?.user.id,
      parentDocumentId: body.parentDocumentId?.trim() ?? null,
      space: body.space,
    });

    return NextResponse.json({ document }, { status: 201 });
  } catch (error) {
    console.error('[planner/documents] create failed', error);
    return NextResponse.json({ error: 'Failed to create planner document' }, { status: 500 });
  }
}