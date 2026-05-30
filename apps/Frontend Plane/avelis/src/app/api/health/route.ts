import { NextResponse } from 'next/server';

export async function GET(): Promise<NextResponse> {
  return NextResponse.json({
    status: 'healthy',
    service: 'avelis-frontend',
    timestamp: new Date().toISOString(),
  });
}