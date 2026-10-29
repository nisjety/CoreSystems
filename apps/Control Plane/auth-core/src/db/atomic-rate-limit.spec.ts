import { atomicIncrementWithExpiry } from './atomic-rate-limit';

describe('atomic Dragonfly rate-limit increment', () => {
  it('increments and starts the expiry in one server-side operation', async () => {
    const evalCommand = jest.fn().mockResolvedValue(3);

    await expect(
      atomicIncrementWithExpiry({ eval: evalCommand }, 'rate:key', 60),
    ).resolves.toBe(3);

    expect(evalCommand).toHaveBeenCalledTimes(1);
    const [script, options] = evalCommand.mock.calls[0] as [
      string,
      { keys: string[]; arguments: string[] },
    ];
    expect(script).toContain("redis.call('INCR', KEYS[1])");
    expect(script).toContain("redis.call('EXPIRE', KEYS[1], ARGV[1])");
    expect(options).toEqual({ keys: ['rate:key'], arguments: ['60'] });
  });

  it('fails closed for invalid input or a malformed server result', async () => {
    const evalCommand = jest.fn().mockResolvedValue('not-a-counter');
    await expect(
      atomicIncrementWithExpiry({ eval: evalCommand }, '', 60),
    ).rejects.toThrow('rate-limit key is required');
    await expect(
      atomicIncrementWithExpiry({ eval: evalCommand }, 'rate:key', 0),
    ).rejects.toThrow('rate-limit ttl must be positive');
    await expect(
      atomicIncrementWithExpiry({ eval: evalCommand }, 'rate:key', 60),
    ).rejects.toThrow('invalid rate-limit counter');
  });
});
