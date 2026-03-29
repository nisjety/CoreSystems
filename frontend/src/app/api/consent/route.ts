import { NextRequest, NextResponse } from 'next/server';
import { ConsentServerActions } from '../../../components/auth/lib/consent/server-cookie';
import logger from '../../../components/auth/lib/server/logger';

export async function GET(req: NextRequest) {
  const ip = req.headers.get('x-forwarded-for') || 'unknown';
  const ua = req.headers.get('user-agent') || '';

  try {
    const consent = await ConsentServerActions.getConsent(req as unknown as Request);
    logger.info('consent.get', { event: 'consent_get', ip, ua, hasConsent: !!consent });
    return NextResponse.json({ consent });
  } catch (err) {
    logger.error('consent.get_failed', { event: 'consent_get_failed', ip, ua, error: (err as Error).message });
    return NextResponse.json({ error: 'Failed to get consent' }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  const ip = req.headers.get('x-forwarded-for') || 'unknown';
  const ua = req.headers.get('user-agent') || '';
  const body = await req.json();

  const { purposes, userId, sessionId } = body || {};
  if (!purposes || !sessionId) {
    return NextResponse.json({ error: 'purposes and sessionId are required' }, { status: 400 });
  }

  try {
    const result = await ConsentServerActions.setConsent(purposes, {
      userId,
      sessionId,
      userAgent: ua,
      ipAddress: Array.isArray(ip) ? ip[0] : ip,
    });
    logger.info('consent.set', { event: 'consent_set', userId, ip, ua, consentId: result.consentId, auditId: result.auditId });
    return NextResponse.json(result);
  } catch (err) {
    logger.error('consent.set_failed', { event: 'consent_set_failed', userId, ip, ua, error: (err as Error).message });
    return NextResponse.json({ error: 'Failed to set consent' }, { status: 500 });
  }
}

export async function PATCH(req: NextRequest) {
  const ip = req.headers.get('x-forwarded-for') || 'unknown';
  const ua = req.headers.get('user-agent') || '';
  const body = await req.json();
  const { purposes, userId, sessionId, method } = body || {};
  if (!purposes || !sessionId) {
    return NextResponse.json({ error: 'purposes and sessionId are required' }, { status: 400 });
  }

  try {
    const result = await ConsentServerActions.updateConsent(req as unknown as Request, purposes, {
      userId,
      sessionId,
      userAgent: ua,
      ipAddress: Array.isArray(ip) ? ip[0] : ip,
      method: method || 'preferences',
    });
    logger.info('consent.update', { event: 'consent_update', userId, ip, ua, consentId: result.consentId });
    return NextResponse.json(result);
  } catch (err) {
    logger.error('consent.update_failed', { event: 'consent_update_failed', userId, ip, ua, error: (err as Error).message });
    return NextResponse.json({ error: 'Failed to update consent' }, { status: 500 });
  }
}

export async function DELETE(req: NextRequest) {
  const ip = req.headers.get('x-forwarded-for') || 'unknown';
  const ua = req.headers.get('user-agent') || '';
  const body = await req.json().catch(() => ({}));
  const { userId, sessionId } = body || {};
  if (!sessionId) {
    return NextResponse.json({ error: 'sessionId is required' }, { status: 400 });
  }

  try {
    const result = await ConsentServerActions.withdrawConsent(req as unknown as Request, {
      userId,
      sessionId,
      userAgent: ua,
      ipAddress: Array.isArray(ip) ? ip[0] : ip,
      method: 'user',
    });
    logger.info('consent.withdraw', { event: 'consent_withdraw', userId, ip, ua });
    return NextResponse.json(result);
  } catch (err) {
    logger.error('consent.withdraw_failed', { event: 'consent_withdraw_failed', userId, ip, ua, error: (err as Error).message });
    return NextResponse.json({ error: 'Failed to withdraw consent' }, { status: 500 });
  }
}