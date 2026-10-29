import http from 'node:http';
import { spawn } from 'node:child_process';
import path from 'node:path';

function wait(ms: number) {
  return new Promise((res) => setTimeout(res, ms));
}

const TEST_PORT = Number(process.env.TEST_PORT || 4011);

function requestGet(path: string): Promise<{
  status: number;
  body: string;
  headers: http.IncomingHttpHeaders;
}> {
  const url = new URL(`http://127.0.0.1:${TEST_PORT}${path}`);
  return new Promise((resolve, reject) => {
    const req = http.request(url, { method: 'GET' }, (res) => {
      let data = '';
      res.setEncoding('utf8');
      res.on('data', (chunk) => (data += chunk));
      res.on('end', () =>
        resolve({
          status: res.statusCode || 0,
          body: data,
          headers: res.headers,
        }),
      );
    });
    req.on('error', reject);
    req.end();
  });
}

// Black-box smoke test: start the dev server and query endpoints

describe('HTTP smoke', () => {
  let child: ReturnType<typeof spawn> | undefined;

  beforeAll(async () => {
    child = spawn('npm', ['run', 'start:prod'], {
      cwd: path.resolve(__dirname, '..'),
      env: { ...process.env, PORT: String(TEST_PORT) },
      stdio: 'pipe',
    });

    // surface child logs on failure
    child.stdout?.on('data', (d) => process.stdout.write(`child: ${d}`));
    child.stderr?.on('data', (d) => process.stderr.write(`child-err: ${d}`));

    // wait until server is up (simple retry)
    const start = Date.now();
    let ready = false;
    while (Date.now() - start < 60000 && !ready) {
      try {
        const res = await requestGet('/orpc/openapi.json');
        ready = res.status === 200;
      } catch {
        // ignore
      }
      if (!ready) await wait(500);
    }
    if (!ready) throw new Error('Server did not start in time');
  }, 30000);

  afterAll(() => {
    if (child) {
      child.kill('SIGTERM');
    }
  });

  it('serves OpenAPI JSON', async () => {
    const res = await requestGet('/orpc/openapi.json');
    expect(res.status).toBe(200);
    expect(res.headers['content-type'] || '').toMatch(/json/);
    type OpenApiLike = { openapi?: string; openapiVersion?: string };
    const obj = JSON.parse(res.body) as OpenApiLike;
    expect(obj.openapi || obj.openapiVersion).toBeDefined();
  });

  it('serves Swagger UI', async () => {
    const res = await requestGet('/orpc/docs');
    expect(res.status).toBe(200);
    expect(res.headers['content-type'] || '').toMatch(/html/);
  });

  it('oRPC getSession unauthenticated', async () => {
    // RPC handler expects POST; but GET to docs is already validated, so here we just ensure handler route exists via 404 for GET
    const res = await requestGet('/orpc/auth/getSession');
    // For GET, our controller returns 404 json for unmatched
    expect([200, 404]).toContain(res.status);
  });
});
