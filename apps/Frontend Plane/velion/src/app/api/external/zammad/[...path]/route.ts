import { NextRequest, NextResponse } from 'next/server'

export async function GET(request: NextRequest, { params }: { params: { path: string[] } }) {
  const path = params.path.join('/')
  const baseUrl = process.env.EXTERNAL_ZAMMAD_API_URL || 'http://localhost:3012'
  const token = process.env.EXTERNAL_ZAMMAD_TOKEN

  if (!token) {
    return NextResponse.json({ error: 'Zammad token not configured' }, { status: 500 })
  }

  try {
    const url = new URL(path, baseUrl)
    url.search = request.nextUrl.search

    const response = await fetch(url, {
      method: 'GET',
      headers: {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error) {
    return NextResponse.json({ error: 'Failed to proxy request' }, { status: 500 })
  }
}

export async function POST(request: NextRequest, { params }: { params: { path: string[] } }) {
  const path = params.path.join('/')
  const baseUrl = process.env.EXTERNAL_ZAMMAD_API_URL || 'http://localhost:3012'
  const token = process.env.EXTERNAL_ZAMMAD_TOKEN
  const body = await request.json()

  if (!token) {
    return NextResponse.json({ error: 'Zammad token not configured' }, { status: 500 })
  }

  try {
    const url = new URL(path, baseUrl)
    url.search = request.nextUrl.search

    const response = await fetch(url, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error) {
    return NextResponse.json({ error: 'Failed to proxy request' }, { status: 500 })
  }
}

export async function PATCH(request: NextRequest, { params }: { params: { path: string[] } }) {
  const path = params.path.join('/')
  const baseUrl = process.env.EXTERNAL_ZAMMAD_API_URL || 'http://localhost:3012'
  const token = process.env.EXTERNAL_ZAMMAD_TOKEN
  const body = await request.json()

  if (!token) {
    return NextResponse.json({ error: 'Zammad token not configured' }, { status: 500 })
  }

  try {
    const url = new URL(path, baseUrl)
    url.search = request.nextUrl.search

    const response = await fetch(url, {
      method: 'PATCH',
      headers: {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error) {
    return NextResponse.json({ error: 'Failed to proxy request' }, { status: 500 })
  }
}

export async function OPTIONS() {
  return new NextResponse(null, {
    status: 200,
    headers: {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, POST, PATCH, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type',
    },
  })
}
