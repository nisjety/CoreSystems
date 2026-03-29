import { NextRequest, NextResponse } from 'next/server'

/**
 * Manually trigger ingestion of a completed Quarry crawl job into Data Plane.
 * POST /api/ingestion/ingest-job
 * Body: { crawlJobId: string, orgId: string, sourceUrl: string }
 */

const QUARRY_URL = process.env.QUARRY_API_URL || 'http://localhost:8092'
// Use Docker internal network name for Data Plane (documents-service container):
// External host (localhost:9401) won't work from inside frontend container
// Must use internal Docker DNS name
const DATAPLANE_URL = process.env.DATAPLANE_API_URL || 'http://data-documents-service:8001'

export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const { crawlJobId, orgId, sourceUrl } = body

    if (!crawlJobId || !orgId || !sourceUrl) {
      return NextResponse.json(
        { error: 'Missing required fields: crawlJobId, orgId, sourceUrl' },
        { status: 400 }
      )
    }

    // 1. Fetch crawl results from Quarry
    const jobResponse = await fetch(`${QUARRY_URL}/v1/jobs/${crawlJobId}`, {
      headers: { 'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345' },
    })

    if (!jobResponse.ok) {
      return NextResponse.json(
        { error: `Failed to fetch crawl job from Quarry: ${jobResponse.status}` },
        { status: jobResponse.status }
      )
    }

    const jobData = await jobResponse.json()
    const products = jobData?.result?.products || []

    if (products.length === 0) {
      return NextResponse.json(
        { error: 'Crawl job returned 0 products' },
        { status: 400 }
      )
    }

    // 2. Convert products to Data Plane document format and ingest
    let ingestedCount = 0
    let failedCount = 0
    const errors: string[] = []

    for (const product of products) {
      const document = {
        org_id: orgId,
        source: `website-crawl:${sourceUrl}`,
        type: 'webpage',
        title: product.title || product.name || 'Untitled',
        content: product.content || '',
        url: product.url || sourceUrl,
        metadata: {
          page_type: product.page_type || 'unknown',
          company_name: product.metadata?.company_name,
          crawl_job_id: crawlJobId,
          extracted_at: new Date().toISOString(),
          ...product.metadata,
        },
      }

      try {
        const ingestResponse = await fetch(`${DATAPLANE_URL}/v1/documents`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(document),
        })

        if (ingestResponse.ok) {
          ingestedCount++
          console.log(`✓ Ingested: ${product.url}`)
        } else {
          failedCount++
          const statusText = ingestResponse.statusText || 'Unknown'
          console.error(
            `✗ Failed to ingest ${product.url}: ${ingestResponse.status} ${statusText}`
          )
          errors.push(`${product.url}: ${ingestResponse.status}`)
        }
      } catch (err) {
        failedCount++
        const message = err instanceof Error ? err.message : String(err)
        console.error(`Error ingesting ${product.url}:`, message)
        errors.push(`${product.url}: ${message}`)
      }
    }

    console.log(`Ingestion complete: ${ingestedCount} success, ${failedCount} failed`)

    console.log(`Ingestion complete: ${ingestedCount} success, ${failedCount} failed`)

    return NextResponse.json({
      success: ingestedCount > 0,
      ingestedCount,
      failedCount,
      totalProducts: products.length,
      errors: errors.length > 0 ? errors : undefined,
      message: `Ingested ${ingestedCount}/${products.length} documents from crawl job ${crawlJobId}`,
    })
  } catch (err) {
    console.error('Ingest job API error:', err)
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    )
  }
}
