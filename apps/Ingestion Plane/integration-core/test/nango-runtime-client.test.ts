import { describe, expect, it } from 'vitest';

import { withConnectApiUrl } from '../src/common/runtime/nango-runtime-client';

describe('withConnectApiUrl', () => {
  it('adds the browser-reachable self-hosted API URL to Nango connect links', () => {
    const result = withConnectApiUrl(
      'http://localhost:3009/?session_token=nango_connect_session_123',
      'http://localhost:3003/'
    );

    expect(result).toBe(
      'http://localhost:3009/?session_token=nango_connect_session_123&apiURL=http%3A%2F%2Flocalhost%3A3003'
    );
  });

  it('does not override an existing apiURL parameter', () => {
    const result = withConnectApiUrl(
      'http://localhost:3009/?session_token=nango_connect_session_123&apiURL=http%3A%2F%2Flocalhost%3A3999',
      'http://localhost:3003'
    );

    expect(result).toBe(
      'http://localhost:3009/?session_token=nango_connect_session_123&apiURL=http%3A%2F%2Flocalhost%3A3999'
    );
  });

  it('keeps links unchanged when no public API URL is configured', () => {
    const link = 'http://localhost:3009/?session_token=nango_connect_session_123';

    expect(withConnectApiUrl(link)).toBe(link);
  });
});
