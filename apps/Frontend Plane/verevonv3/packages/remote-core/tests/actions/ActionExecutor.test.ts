import { describe, expect, it } from 'vitest';
import { ActionExecutor } from '../../src/actions/ActionExecutor.js';
import { PermissionManager } from '../../src/permissions/PermissionManager.js';
import { PermissionDeniedError } from '../../src/errors/RemoteError.js';
import { MockProtocol } from '../doubles/MockProtocol.js';

describe('ActionExecutor', () => {
  it('rejects an action when the required permission is missing', async () => {
    const protocol = new MockProtocol();
    const permissions = new PermissionManager([]);
    const executor = new ActionExecutor(protocol, permissions);

    await expect(executor.execute({ type: 'pointer.move', actor: 'human', x: 1, y: 1 })).rejects.toThrow(
      PermissionDeniedError,
    );
    expect(protocol.sentActions).toHaveLength(0);
  });

  it('forwards a permitted action to the protocol', async () => {
    const protocol = new MockProtocol();
    const permissions = new PermissionManager(['input.pointer']);
    const executor = new ActionExecutor(protocol, permissions);

    const result = await executor.execute({ type: 'pointer.move', actor: 'human', x: 5, y: 6 });

    expect(result.ok).toBe(true);
    expect(protocol.sentActions).toEqual([{ type: 'pointer.move', actor: 'human', x: 5, y: 6 }]);
  });

  it('runs middleware around the protocol call, in registration order', async () => {
    const protocol = new MockProtocol();
    const permissions = new PermissionManager(['input.keyboard']);
    const executor = new ActionExecutor(protocol, permissions);
    const calls: string[] = [];

    executor.use(async (_action, next) => {
      calls.push('outer:before');
      const result = await next();
      calls.push('outer:after');
      return result;
    });
    executor.use(async (_action, next) => {
      calls.push('inner:before');
      const result = await next();
      calls.push('inner:after');
      return result;
    });

    await executor.execute({ type: 'keyboard.keyDown', actor: 'ai', key: 'Enter' });

    expect(calls).toEqual(['outer:before', 'inner:before', 'inner:after', 'outer:after']);
  });

  it('lets middleware deny an AI action without touching the protocol', async () => {
    const protocol = new MockProtocol();
    const permissions = new PermissionManager(['input.pointer']);
    const executor = new ActionExecutor(protocol, permissions);

    executor.use(async (action) => {
      if (action.actor === 'ai') {
        return { ok: false, action };
      }
      throw new Error('unreachable in this test');
    });

    const result = await executor.execute({ type: 'pointer.click', actor: 'ai', x: 1, y: 1, button: 'left' });

    expect(result.ok).toBe(false);
    expect(protocol.sentActions).toHaveLength(0);
  });
});
