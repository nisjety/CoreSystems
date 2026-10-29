import { fetchReconciliation } from './reconciliation-http';

describe('reconciliation HTTP', () => {
  afterEach(() => {
    jest.useRealTimers();
    jest.restoreAllMocks();
  });

  it('aborts an upstream reconciliation request at the configured deadline', async () => {
    jest.useFakeTimers();
    const fetchMock = jest.spyOn(global, 'fetch').mockImplementation(
      (_input, init) =>
        new Promise((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => {
            reject(new DOMException('request timed out', 'AbortError'));
          });
        }),
    );

    const request = fetchReconciliation('http://org-core/internal/reconcile', {
      method: 'POST',
    });
    const rejection = expect(request).rejects.toMatchObject({
      name: 'AbortError',
    });

    await jest.advanceTimersByTimeAsync(5_000);
    await rejection;
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][1]?.signal?.aborted).toBe(true);
  });

  it('clears the deadline after a completed request', async () => {
    jest.useFakeTimers();
    jest
      .spyOn(global, 'fetch')
      .mockResolvedValue(new Response(null, { status: 204 }));

    await expect(
      fetchReconciliation('http://org-core/internal/reconcile'),
    ).resolves.toMatchObject({ status: 204 });
    expect(jest.getTimerCount()).toBe(0);
  });

  it('never forwards scoped reconciliation credentials through redirects', async () => {
    const fetchMock = jest
      .spyOn(global, 'fetch')
      .mockResolvedValue(new Response(null, { status: 307 }));

    await fetchReconciliation('http://org-core/internal/reconcile', {
      method: 'POST',
      headers: { 'x-service-token': 'scoped-token' },
    });

    expect(fetchMock.mock.calls[0][1]?.redirect).toBe('manual');
  });
});
