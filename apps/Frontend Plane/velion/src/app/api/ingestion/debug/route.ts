import { NextResponse } from 'next/server';

export async function GET() {
    try {
        const QUARRY_URL = process.env.QUARRY_API_URL || 'http://localhost:9090';
        console.log("Testing reachability to:", QUARRY_URL);

        // Testing health fetch
        const health = await fetch(`${QUARRY_URL}/health`);
        const healthText = await health.text();
        console.log("Health returned:", health.status, healthText);

        // Testing API fetch
        const response = await fetch(`${QUARRY_URL}/v1/jobs?limit=5`, {
            method: 'GET',
            headers: {
                'Accept': 'application/json',
                'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345',
            },
            signal: AbortSignal.timeout(5000),
        });

        const text = await response.text();
        return NextResponse.json({
            success: true,
            status: response.status,
            body: text,
            env: QUARRY_URL
        });
    } catch (err: any) {
        console.error("DEBUG ROUTE ERR:", err);
        return NextResponse.json({
            error: 'Fetch failed',
            message: err.message,
            cause: err.cause ? String(err.cause) : null,
            stack: err.stack
        }, { status: 500 });
    }
}
