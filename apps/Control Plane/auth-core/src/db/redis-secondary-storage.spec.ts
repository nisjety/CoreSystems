const mockRedis = {
  on: jest.fn(),
  isOpen: true,
  get: jest.fn(),
};

jest.mock('redis', () => ({
  createClient: () => mockRedis,
}));

import { redisSecondaryStorage } from './redis';

describe('Dragonfly secondary storage failure semantics', () => {
  it('propagates read failures so authentication fails operationally, not as absent', async () => {
    mockRedis.get.mockRejectedValue(new Error('cache unavailable'));

    await expect(redisSecondaryStorage.get('session:key')).rejects.toThrow(
      'cache unavailable',
    );
  });
});
