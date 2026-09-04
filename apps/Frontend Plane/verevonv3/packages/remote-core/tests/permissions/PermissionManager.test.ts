import { describe, expect, it, vi } from 'vitest';
import { PermissionManager } from '../../src/permissions/PermissionManager.js';
import { PermissionDeniedError } from '../../src/errors/RemoteError.js';

describe('PermissionManager', () => {
  it('reports granted permissions', () => {
    const manager = new PermissionManager(['screen.view']);
    expect(manager.has('screen.view')).toBe(true);
    expect(manager.has('input.pointer')).toBe(false);
  });

  it('require() throws PermissionDeniedError when missing', () => {
    const manager = new PermissionManager([]);
    expect(() => manager.require('input.keyboard')).toThrow(PermissionDeniedError);
  });

  it('require() does not throw when granted', () => {
    const manager = new PermissionManager(['input.keyboard']);
    expect(() => manager.require('input.keyboard')).not.toThrow();
  });

  it('update() notifies subscribers only when the set actually changes', () => {
    const manager = new PermissionManager(['screen.view']);
    const handler = vi.fn();
    manager.onChange(handler);

    manager.update(['screen.view']);
    expect(handler).not.toHaveBeenCalled();

    manager.update(['screen.view', 'input.pointer']);
    expect(handler).toHaveBeenCalledWith(['screen.view', 'input.pointer']);
  });

  it('never assumes input permission from screen.view', () => {
    const manager = new PermissionManager(['screen.view']);
    expect(() => manager.require('input.pointer')).toThrow(PermissionDeniedError);
  });
});
