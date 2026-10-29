import { GET } from '../../../../src/app/api/health/route';

describe('GET /api/health', () => {
  it('returns a healthy payload', async () => {
    const response = await GET();
    const data = await response.json();

    expect(response.status).toBe(200);
    expect(data.status).toBe('healthy');
    expect(data.service).toBe('avelis-frontend');
    expect(typeof data.timestamp).toBe('string');
  });
});