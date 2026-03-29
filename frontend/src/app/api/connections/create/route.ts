import { NextRequest, NextResponse } from 'next/server'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://localhost:9026'

export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const { org_id, user_id, sources } = body

    if (!org_id || !user_id || !Array.isArray(sources)) {
      return NextResponse.json(
        { error: 'Missing org_id, user_id, or sources' },
        { status: 400 }
      )
    }

    // Validate sources
    const validSources = ['sharepoint', 'onedrive', 'teams', 'outlook']
    const filteredSources = sources.filter((s) => validSources.includes(s.toLowerCase()))

    if (filteredSources.length === 0) {
      return NextResponse.json(
        { success: true, connections_created: 0, documents_discovered: 0 }
      )
    }

    // Call integration-core to create connections
    const response = await fetch(`${INTEGRATION_CORE_URL}/api/v1/connections/create`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        org_id,
        user_id,
        sources: filteredSources,
      }),
    })

    if (!response.ok) {
      console.error(`Integration-core returned ${response.status}`)
      const errorText = await response.text()
      console.error('Error:', errorText)
      // Don't fail; connections might partially succeed
      return NextResponse.json(
        {
          success: false,
          connections_created: 0,
          documents_discovered: 0,
          warning: 'Failed to create all connections',
        },
        { status: 500 }
      )
    }

    const data = await response.json()
    return NextResponse.json(data)
  } catch (error) {
    console.error('Create connections error:', error)
    return NextResponse.json(
      {
        success: false,
        connections_created: 0,
        documents_discovered: 0,
        error: error instanceof Error ? error.message : 'Internal server error',
      },
      { status: 500 }
    )
  }
}
