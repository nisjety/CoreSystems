import { describe, expect, it } from 'vitest';
import { CoordinateMapper } from '../../src/renderer/CoordinateMapper.js';

describe('CoordinateMapper', () => {
  it('maps 1:1 when remote and viewport sizes match', () => {
    const mapper = new CoordinateMapper({ width: 1920, height: 1080 }, { width: 1920, height: 1080 });
    expect(mapper.remoteToLocal({ x: 100, y: 200 })).toEqual({ x: 100, y: 200 });
    expect(mapper.localToRemote({ x: 100, y: 200 })).toEqual({ x: 100, y: 200 });
  });

  it('letterboxes on the horizontal axis when the viewport is wider than the remote aspect ratio', () => {
    const mapper = new CoordinateMapper({ width: 800, height: 600 }, { width: 1600, height: 600 });
    expect(mapper.remoteToLocal({ x: 0, y: 0 })).toEqual({ x: 400, y: 0 });
    expect(mapper.remoteToLocal({ x: 800, y: 600 })).toEqual({ x: 1200, y: 600 });
  });

  it('clamps localToRemote to the remote bounds inside the letterbox padding', () => {
    const mapper = new CoordinateMapper({ width: 800, height: 600 }, { width: 1600, height: 600 });
    expect(mapper.localToRemote({ x: 0, y: 0 })).toEqual({ x: 0, y: 0 });
    expect(mapper.localToRemote({ x: 1600, y: 600 })).toEqual({ x: 800, y: 600 });
  });

  it('recomputes after update()', () => {
    const mapper = new CoordinateMapper({ width: 100, height: 100 }, { width: 100, height: 100 });
    mapper.update({ width: 200, height: 200 }, { width: 100, height: 100 });
    expect(mapper.getScale()).toBeCloseTo(0.5);
  });
});
