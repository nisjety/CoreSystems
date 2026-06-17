'use client';

import React, { useRef, useEffect, useState } from 'react';
import { outerVertex, outerFragment, innerVertex, innerFragment } from './orb-shaders';
import { getCanvasWebGLContext, createHaloTexture, fillSpherePoints } from './orb-helpers';
import {
  getPalette,
  getAnimDynamics,
  getScaleModulation,
  getCoreWobble,
  getCoreScalePulse,
  getDeformModulation,
  getParticleSwirl,
  getRadialPush,
  getIntensityModulation,
} from './orb-state-config';

export type OrbAnimationState = 'waiting' | 'listening' | 'thinking' | 'responding';

interface ThreeJSOrbProps {
  state: OrbAnimationState;
  size?: number;
  className?: string;
  onStateChange?: (state: OrbAnimationState) => void;
  /**
   * Skip WebGL entirely and always render the CSS fallback. Use for
   * small repeated instances (e.g. per-message avatars) — browsers cap
   * live WebGL contexts at ~8–16 and start evicting the oldest once that
   * fills up, which would re-mount React components and drop state.
   */
  forceFallback?: boolean;
}

/* ── Apply palette to live uniforms ──────────────────────────────────── */

function applyPalette(
  state: OrbAnimationState,
  outerUniforms: any,
  innerUniforms: any,
  particles: import('three').Points | null,
  baseValues: { intensity: number; deform: number },
) {
  const p = getPalette(state);

  outerUniforms.uBase.value.set(...p.outer.base);
  outerUniforms.uAccent1.value.set(...p.outer.accent1);
  outerUniforms.uAccent2.value.set(...p.outer.accent2);
  outerUniforms.uCore.value.set(...p.outer.core);
  outerUniforms.uIntensity.value = p.outer.intensity;
  outerUniforms.uGlow.value = p.outer.glow;
  outerUniforms.uOpacity.value = p.outer.opacity;
  baseValues.intensity = p.outer.intensity;

  innerUniforms.uDeform.value = p.inner.deform;
  innerUniforms.uOpacity.value = p.inner.opacity;
  innerUniforms.uColor.value.set(...p.inner.color);
  baseValues.deform = p.inner.deform;

  if (particles) {
    const pm = particles.material as import('three').PointsMaterial;
    if (typeof p.particle.color === 'number') {
      pm.color.set(p.particle.color);
    } else {
      pm.color.setStyle(p.particle.color);
    }
    pm.opacity = p.particle.opacity;
  }
}

/* ═══════════════════════════════════════════════════════════════════════ */

export const ThreeJSOrb: React.FC<ThreeJSOrbProps> = ({
  state,
  size = 120,
  className = '',
  onStateChange,
  forceFallback = false,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const animationIdRef = useRef<number | null>(null);
  const threeRef = useRef<typeof import('three') | null>(null);

  const outerUniformsRef = useRef<any>(null);
  const innerUniformsRef = useRef<any>(null);
  const particlesRef = useRef<import('three').Points | null>(null);
  const orbMeshRef = useRef<import('three').Mesh | null>(null);
  const coreMeshRef = useRef<import('three').Mesh | null>(null);
  const rendererRef = useRef<import('three').WebGLRenderer | null>(null);
  const sceneRef = useRef<import('three').Scene | null>(null);
  const cameraRef = useRef<import('three').PerspectiveCamera | null>(null);

  const [isReady, setIsReady] = useState(false);
  const [hasRendererFailure, setHasRendererFailure] = useState(false);

  const stateRef = useRef<OrbAnimationState>(state);
  useEffect(() => { stateRef.current = state; }, [state]);

  const baseValuesRef = useRef<{ intensity: number; deform: number }>({ intensity: 0.2, deform: 0.2 });

  // ── Lazy-import Three ─────────────────────────────────────────────────
  useEffect(() => {
    if (forceFallback) return;
    (async () => {
      try {
        const THREE = await import('three');
        threeRef.current = THREE;
        setIsReady(true);
      } catch {
        setHasRendererFailure(true);
      }
    })();
  }, [forceFallback]);

  // ── Init scene ────────────────────────────────────────────────────────
  useEffect(() => {
    if (!isReady || hasRendererFailure || !containerRef.current) return;
    const THREE = threeRef.current!;
    const container = containerRef.current;
    let canvas: HTMLCanvasElement | null = null;
    let renderer: import('three').WebGLRenderer | null = null;
    let haloTex: import('three').CanvasTexture | null = null;
    let outerGeo: import('three').SphereGeometry | null = null;
    let coreGeo: import('three').IcosahedronGeometry | null = null;
    let scene: import('three').Scene | null = null;
    let camera: import('three').PerspectiveCamera | null = null;
    let outerUniforms: any = null;
    let innerUniforms: any = null;

    try {
      canvas = document.createElement('canvas');
      canvas.style.width = `${size}px`;
      canvas.style.height = `${size}px`;
      canvas.width = size;
      canvas.height = size;
      canvas.style.borderRadius = '50%';
      canvas.style.display = 'block';
      container.appendChild(canvas);
      canvasRef.current = canvas;

      scene = new THREE.Scene();
      sceneRef.current = scene;

      camera = new THREE.PerspectiveCamera(30, 1, 0.1, 100);
      camera.position.set(0, 0, 3.3);
      cameraRef.current = camera;

      const webglContext = getCanvasWebGLContext(canvas);
      if (!webglContext) throw new Error('WebGL unavailable');

      renderer = new THREE.WebGLRenderer({ canvas, context: webglContext, alpha: true, antialias: true, powerPreference: 'low-power' });
      renderer.shadowMap.enabled = true;
      renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      renderer.setSize(size, size, false);
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      rendererRef.current = renderer;

      // Lights
      const dir = new THREE.DirectionalLight(0xffffff, 0.7);
      dir.position.set(2.5, 3.0, 3.0);
      dir.castShadow = true;
      dir.shadow.mapSize.set(512, 512);
      scene.add(dir);
      scene.add(new THREE.AmbientLight(0xffffff, 0.35));

      // Shadow receiver
      const receiver = new THREE.Mesh(
        new THREE.PlaneGeometry(6, 6),
        new THREE.ShadowMaterial({ opacity: 0.22 }),
      );
      receiver.receiveShadow = true;
      receiver.position.set(0, -1.15, 0);
      receiver.rotation.x = -Math.PI / 2;
      scene.add(receiver);

      // Halo sprite
      haloTex = new THREE.CanvasTexture(createHaloTexture());
      haloTex.anisotropy = 4;
      const halo = new THREE.Sprite(new THREE.SpriteMaterial({ map: haloTex, transparent: true, depthWrite: false }));
      halo.scale.set(2.8, 2.8, 1);
      halo.position.set(0, 0, -0.08);
      scene.add(halo);

      // Outer orb
      outerGeo = new THREE.SphereGeometry(0.98, 72, 72);
      outerUniforms = {
        uTime:      { value: 0 },
        uIntensity: { value: 0.2 },
        uGlow:      { value: 0.85 },
        uOpacity:   { value: 0.94 },
        uBase:      { value: new THREE.Vector3(0.70, 0.76, 1.00) },
        uAccent1:   { value: new THREE.Vector3(0.86, 0.90, 1.00) },
        uAccent2:   { value: new THREE.Vector3(0.94, 0.86, 1.00) },
        uCore:      { value: new THREE.Vector3(1.0, 1.0, 1.0) },
      };
      outerUniformsRef.current = outerUniforms;

      const outerMat = new THREE.ShaderMaterial({
        vertexShader: outerVertex,
        fragmentShader: outerFragment,
        uniforms: outerUniforms,
        transparent: true,
        depthWrite: false,
        blending: THREE.NormalBlending,
      });
      const orb = new THREE.Mesh(outerGeo, outerMat);
      orb.castShadow = true;
      orbMeshRef.current = orb;
      scene.add(orb);

      // Inner core
      coreGeo = new THREE.IcosahedronGeometry(0.5, 4);
      innerUniforms = {
        uTime:    { value: 0 },
        uDeform:  { value: 0.22 },
        uOpacity: { value: 0.92 },
        uColor:   { value: new THREE.Vector3(1.0, 1.0, 1.0) },
      };
      innerUniformsRef.current = innerUniforms;
      const innerMat = new THREE.ShaderMaterial({
        vertexShader: innerVertex,
        fragmentShader: innerFragment,
        uniforms: innerUniforms,
        transparent: true,
        depthWrite: false,
      });
      const core = new THREE.Mesh(coreGeo, innerMat);
      core.castShadow = true;
      coreMeshRef.current = core;
      scene.add(core);

      // Particles
      const pGeo = fillSpherePoints(THREE, 900, 0.9);
      const pMat = new THREE.PointsMaterial({
        size: 0.018, sizeAttenuation: true, transparent: true,
        color: 0xffffff, opacity: 0.65, depthWrite: false,
      });
      const points = new THREE.Points(pGeo, pMat);
      particlesRef.current = points;
      scene.add(points);
    } catch {
      setHasRendererFailure(true);
      renderer?.dispose();
      haloTex?.dispose();
      outerGeo?.dispose();
      coreGeo?.dispose();
      if (canvas && container.contains(canvas)) container.removeChild(canvas);
      canvasRef.current = null;
      rendererRef.current = null;
      sceneRef.current = null;
      cameraRef.current = null;
      orbMeshRef.current = null;
      coreMeshRef.current = null;
      particlesRef.current = null;
      outerUniformsRef.current = null;
      innerUniformsRef.current = null;
      return;
    }

    if (!renderer || !scene || !camera || !outerUniforms || !innerUniforms) {
      setHasRendererFailure(true);
      return;
    }

    // Apply initial palette
    applyPalette(state, outerUniforms, innerUniforms, particlesRef.current, baseValuesRef.current);

    // ── Animation loop ──────────────────────────────────────────────────
    const clock = new THREE.Clock();
    const animate = () => {
      const dt = clock.getDelta();
      outerUniforms.uTime.value += dt;
      innerUniforms.uTime.value += dt;

      const t = outerUniforms.uTime.value;
      const st = stateRef.current;
      const { baseSpin, spinX } = getAnimDynamics(st);
      const scaleMod = getScaleModulation(st, t);

      if (orbMeshRef.current) {
        orbMeshRef.current.scale.setScalar(1.0 + scaleMod);
        orbMeshRef.current.rotation.y += baseSpin * dt;
        orbMeshRef.current.rotation.x += spinX * dt;
        if (st === 'thinking' || st === 'responding') {
          orbMeshRef.current.rotation.z += (st === 'thinking' ? 0.35 : 0.55) * dt;
        }
      }

      if (coreMeshRef.current) {
        const wobble = getCoreWobble(st, t);
        coreMeshRef.current.rotation.y -= baseSpin * 0.66 * dt;
        coreMeshRef.current.rotation.x += spinX * 0.58 * dt + wobble * dt;
        coreMeshRef.current.rotation.z += (st === 'responding' ? 0.55 : 0.28) * dt;
        coreMeshRef.current.scale.setScalar(1.0 + getCoreScalePulse(st, t));

        if (innerUniformsRef.current) {
          innerUniformsRef.current.uDeform.value =
            baseValuesRef.current.deform + getDeformModulation(st, t);
        }
      }

      if (particlesRef.current) {
        const geo = particlesRef.current.geometry as import('three').BufferGeometry;
        const pos = geo.getAttribute('position') as import('three').BufferAttribute;
        const count = pos.count;
        const swirl = getParticleSwirl(st) * dt;
        const pushMul = getRadialPush(st);

        for (let i = 0; i < count; i++) {
          const ix = i * 3;
          const x = pos.array[ix + 0];
          const z = pos.array[ix + 2];
          const r = Math.hypot(x, z) + 1e-6;
          const ang = Math.atan2(z, x) + swirl * (0.38 + (i % 5) * 0.01);
          pos.array[ix + 0] = Math.cos(ang) * r;
          pos.array[ix + 2] = Math.sin(ang) * r;

          const y = pos.array[ix + 1];
          const rr = Math.sqrt(x * x + y * y + z * z);
          const push = Math.sin(t * 1.25 + i * 0.011) * 0.0009 * pushMul;
          const f = rr > 0.0001 ? (rr + push) / rr : 1.0;
          pos.array[ix + 0] *= f;
          pos.array[ix + 1] *= f;
          pos.array[ix + 2] *= f;
        }
        pos.needsUpdate = true;
      }

      if (outerUniformsRef.current) {
        outerUniformsRef.current.uIntensity.value =
          baseValuesRef.current.intensity + getIntensityModulation(st, t);
      }

      renderer.render(scene, camera);
      animationIdRef.current = requestAnimationFrame(animate);
    };
    animate();

    return () => {
      if (animationIdRef.current) cancelAnimationFrame(animationIdRef.current);
      renderer.dispose();
      haloTex?.dispose();
      outerGeo?.dispose();
      coreGeo?.dispose();
      if (canvas && container.contains(canvas)) container.removeChild(canvas);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasRendererFailure, isReady, size]);

  // ── State changes → re-apply palette + auto-transitions ───────────────
  useEffect(() => {
    const ou = outerUniformsRef.current;
    const iu = innerUniformsRef.current;
    if (!ou || !iu) return;

    applyPalette(state, ou, iu, particlesRef.current, baseValuesRef.current);

    if (onStateChange) {
      const ms = state === 'listening' ? 2000 : state === 'responding' ? 3000 : 0;
      if (ms > 0) {
        const t = setTimeout(() => {
          if (state === 'listening') onStateChange('thinking');
          if (state === 'responding') onStateChange('waiting');
        }, ms);
        return () => clearTimeout(t);
      }
    }
  }, [state, onStateChange]);

  // ── Render ────────────────────────────────────────────────────────────
  if (forceFallback || !isReady || hasRendererFailure) {
    return (
      <div
        className={`three-orb-fallback ${className}`}
        style={{
          width: size,
          height: size,
          borderRadius: '50%',
          background:
            'radial-gradient(60% 60% at 50% 50%, rgba(255,255,255,0.95) 0%, rgba(207,214,255,0.75) 55%, rgba(143,160,255,0.60) 100%)',
          boxShadow: '0 10px 30px rgba(0,0,0,0.18), 0 0 30px rgba(180,190,255,0.45)',
          animation: 'orb-breathe 3.2s ease-in-out infinite',
        }}
        title={`AI-assistent – ${state}`}
      />
    );
  }

  return (
    <div
      ref={containerRef}
      className={`three-orb ${className}`}
      style={{
        width: size,
        height: size,
        borderRadius: '50%',
        filter: 'drop-shadow(0 10px 28px rgba(0,0,0,0.20)) drop-shadow(0 0 22px rgba(180,190,255,0.45))',
        transition: 'transform 0.25s ease',
      }}
      title={`AI-assistent – ${state}`}
    />
  );
};
