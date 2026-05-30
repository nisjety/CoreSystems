import { NextRequest, NextResponse } from 'next/server';
import {
  getPlannerDocumentState,
  savePlannerDocumentState,
} from '@/lib/convex/planner-server';

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ documentId: string }> }
) {
  try {
    const { documentId } = await params;
    const workspaceId = request.nextUrl.searchParams.get('workspaceId')?.trim();

    if (!workspaceId) {
      return NextResponse.json({ error: 'workspaceId is required' }, { status: 400 });
    }

    const state = await getPlannerDocumentState({ workspaceId, documentId });
    if (!state) {
      return new NextResponse(null, { status: 204 });
    }

    const binary = Buffer.from(state.stateBase64, 'base64');

    return new NextResponse(binary, {
      status: 200,
      headers: {
        'Content-Type': 'application/octet-stream',
        'X-Updated-At': String(state.updatedAt),
      },
    });
  } catch (error) {
    console.error('[planner/document/state] get failed', error);
    return NextResponse.json({ error: 'Failed to load planner document state' }, { status: 500 });
  }
}

export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ documentId: string }> }
) {
  try {
    const { documentId } = await params;
    const workspaceId = request.nextUrl.searchParams.get('workspaceId')?.trim();

    if (!workspaceId) {
      return NextResponse.json({ error: 'workspaceId is required' }, { status: 400 });
    }

    const state = await request.arrayBuffer();
    if (state.byteLength === 0) {
      return NextResponse.json({ error: 'state payload is required' }, { status: 400 });
    }

    const result = await savePlannerDocumentState({
      workspaceId,
      documentId,
      stateBase64: Buffer.from(state).toString('base64'),
    });

    return NextResponse.json(result, { status: 202 });
  } catch (error) {
    console.error('[planner/document/state] save failed', error);
    return NextResponse.json({ error: 'Failed to save planner document state' }, { status: 500 });
  }
}