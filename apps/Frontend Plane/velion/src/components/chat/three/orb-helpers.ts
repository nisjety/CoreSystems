/**
 * WebGL helper utilities for the ThreeJS Orb component.
 */

/** Convert any CSS colour (including oklch()) to normalised sRGB via a temp DOM element. */
export function cssColorToRGB(cssColor: string): { r: number; g: number; b: number } | null {
  const el = document.createElement('div');
  el.style.color = cssColor;
  el.style.display = 'none';
  document.body.appendChild(el);
  const cs = getComputedStyle(el).color;
  document.body.removeChild(el);
  const m = cs.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
  if (!m) return null;
  return { r: parseInt(m[1], 10) / 255, g: parseInt(m[2], 10) / 255, b: parseInt(m[3], 10) / 255 };
}

/** Read a CSS custom property and convert OKLCH → sRGB via the browser. */
export function oklchVarToRGB(varName = '--primary', fallback = 'oklch(0.30 0.13 270)') {
  const root = document.documentElement;
  const raw = getComputedStyle(root).getPropertyValue(varName).trim();
  const color = raw || fallback;
  return cssColorToRGB(color) ?? cssColorToRGB('rgb(21, 31, 108)')!;
}

/** Try to obtain a WebGL(2) context from a canvas. */
export function getCanvasWebGLContext(canvas: HTMLCanvasElement) {
  const contextOptions: WebGLContextAttributes = {
    alpha: true,
    antialias: true,
    powerPreference: 'low-power',
  };

  return (
    canvas.getContext('webgl2', contextOptions) ||
    canvas.getContext('webgl', contextOptions) ||
    canvas.getContext('experimental-webgl', contextOptions)
  ) as WebGL2RenderingContext | WebGLRenderingContext | null;
}

/** Create a radial-gradient canvas texture for the halo sprite. */
export function createHaloTexture(): HTMLCanvasElement {
  const size = 256;
  const c = document.createElement('canvas');
  c.width = c.height = size;
  const ctx = c.getContext('2d')!;
  const g = ctx.createRadialGradient(size / 2, size / 2, size * 0.1, size / 2, size / 2, size * 0.6);
  g.addColorStop(0.0, 'rgba(255,255,255,0.55)');
  g.addColorStop(0.6, 'rgba(220,230,255,0.18)');
  g.addColorStop(1.0, 'rgba(220,230,255,0.00)');
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, size, size);
  return c;
}

/** Fill a BufferGeometry with uniformly-distributed sphere points. */
export function fillSpherePoints(THREE: typeof import('three'), count: number, radius: number) {
  const pos = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    let x = 0, y = 0, z = 0;
    do {
      x = Math.random() * 2 - 1;
      y = Math.random() * 2 - 1;
      z = Math.random() * 2 - 1;
    } while (x * x + y * y + z * z > 1);
    const r = radius * Math.cbrt(Math.random());
    pos[i * 3 + 0] = x * r;
    pos[i * 3 + 1] = y * r;
    pos[i * 3 + 2] = z * r;
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  return geo;
}
