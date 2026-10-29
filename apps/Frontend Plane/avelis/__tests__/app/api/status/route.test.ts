import { NextRequest } from 'next/server';

import { GET } from '../../../../src/app/api/status/route';
import { getSessionUser } from '../../../../src/lib/auth-session';

jest.mock('../../../../src/lib/auth-session', () => ({
  getSessionUser: jest.fn(),
}));

describe('GET /api/status', () => {
  afterEach(() => {
    jest.clearAllMocks();
  });

  it('returns 401 without a session user', async () => {
    (getSessionUser as jest.Mock).mockResolvedValue(null);

    const request = new NextRequest('http://localhost/api/status');
    const response = await GET(request);
    const data = await response.json();

    expect(response.status).toBe(401);
    expect(data.error).toBe('Unauthorized');
  });

  it('returns session status for an authenticated user', async () => {
    (getSessionUser as jest.Mock).mockResolvedValue({
      id: 'user-1',
      email: 'test@example.com',
    });

    const request = new NextRequest('http://localhost/api/status');
    const response = await GET(request);
    const data = await response.json();

    expect(response.status).toBe(200);
    expect(data.authenticated).toBe(true);
    expect(data.user.id).toBe('user-1');
  });
});