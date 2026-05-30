import { NextResponse } from 'next/server'
import { listChannelConfigs } from '@/lib/notifications/client'

export async function GET() {
  try {
    const configs = await listChannelConfigs()
    return NextResponse.json({ configs })
  } catch (error) {
    console.error('[api/notifications/channels] Failed:', error)
    return NextResponse.json({ error: 'Failed to fetch channel config' }, { status: 500 })
  }
}
