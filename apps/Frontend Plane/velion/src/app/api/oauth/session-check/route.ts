import { NextRequest, NextResponse } from 'next/server'

export async function GET(request: NextRequest) {
  void request
  return NextResponse.json({
    authenticated: false,
    has_microsoft: false,
    has_google: false,
    error: 'Session-based provider reuse is disabled. Use an Aqencia integration session instead.',
  })
}
