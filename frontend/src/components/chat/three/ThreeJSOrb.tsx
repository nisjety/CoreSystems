'use client';

import React, { useRef, useEffect, useState } from 'react';

export type OrbAnimationState = 'waiting' | 'listening' | 'thinking' | 'responding';

interface ThreeJSOrbProps {
  state: OrbAnimationState;
  size?: number;
  className?: string;
  onStateChange?: (state: OrbAnimationState) => void;
}

/* -------------------------------------------------------
   Hjelpere: hent sRGB fra CSS-OKLCH for indre kjernefarge
   ------------------------------------------------------- */

/** Norsk: Hent computed RGB fra en vilkårlig CSS-farge (inkl. oklch()) */
function cssColorToRGB(cssColor: string): { r: number; g: number; b: number } | null {
  // Lag et midlertidig element og les computed style
  const el = document.createElement('div');
  el.style.color = cssColor;
  el.style.display = 'none';
  document.body.appendChild(el);
  const cs = getComputedStyle(el).color; // typisk "rgb(r, g, b)" eller "rgba(...)"
  document.body.removeChild(el);
  const m = cs.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
  if (!m) return null;
  return { r: parseInt(m[1], 10) / 255, g: parseInt(m[2], 10) / 255, b: parseInt(m[3], 10) / 255 };
}

/** Norsk: Les CSS-variabel og konverter OKLCH→sRGB (via browserens fargestøtte). */
function oklchVarToRGB(varName = '--primary', fallback = 'oklch(0.30 0.13 270)') {
  const root = document.documentElement;
  const raw = getComputedStyle(root).getPropertyValue(varName).trim();
  const color = raw || fallback;
  return cssColorToRGB(color) ?? cssColorToRGB('rgb(21, 31, 108)')!; // fallback mørk indigo
}

function getCanvasWebGLContext(canvas: HTMLCanvasElement) {
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

/* =======================================================
   ThreeJSOrb
   ======================================================= */

export const ThreeJSOrb: React.FC<ThreeJSOrbProps> = ({
  state,
  size = 120,
  className = '',
  onStateChange,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const animationIdRef = useRef<number | null>(null);
  const threeRef = useRef<typeof import('three') | null>(null);

  // refs for uniforms/materialer
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
  // Track latest state inside animation loop (avoid stale closure)
  const stateRef = useRef<OrbAnimationState>(state);
  useEffect(() => { stateRef.current = state; }, [state]);

  // Store base intensities so we can modulate around them dynamically
  const baseValuesRef = useRef<{ intensity: number; deform: number }>({ intensity: 0.2, deform: 0.2 });

  // Lazy-import av Three
  useEffect(() => {
    (async () => {
      try {
        const THREE = await import('three');
        threeRef.current = THREE;
        setIsReady(true);
      } catch (e) {
        setHasRendererFailure(true);
      }
    })();
  }, []);

  /* ------------------ Shaders ------------------ */

  // YTRE – Fresnel + myk “fluid”-lys (2D simplex)
  const outerVertex = `
    varying vec3 vWorldPosition;
    varying vec3 vNormal;
    varying vec2 vUv;
    void main(){
      vUv = uv;
      vNormal = normalize(normalMatrix * normal);
      vec4 wp = modelMatrix * vec4(position,1.0);
      vWorldPosition = wp.xyz;
      gl_Position = projectionMatrix * viewMatrix * wp;
    }
  `;

  const outerFragment = `
    precision highp float;
    varying vec3 vWorldPosition;
    varying vec3 vNormal;
    varying vec2 vUv;

    uniform float uTime;
    uniform float uIntensity;
    uniform float uGlow;
    uniform float uOpacity;

    uniform vec3 uBase;
    uniform vec3 uAccent1;
    uniform vec3 uAccent2;
    uniform vec3 uCore;

    // 2D simplex
    vec3 mod289(vec3 x){ return x - floor(x*(1.0/289.0))*289.0; }
    vec2 mod289(vec2 x){ return x - floor(x*(1.0/289.0))*289.0; }
    vec3 permute(vec3 x){ return mod289(((x*34.0)+1.0)*x); }
    float snoise(vec2 v){
      const vec4 C=vec4(0.211324865405187,0.366025403784439,-0.577350269189626,0.024390243902439);
      vec2 i=floor(v+dot(v,C.yy));
      vec2 x0=v-i+dot(i,C.xx);
      vec2 i1=(x0.x>x0.y)?vec2(1.0,0.0):vec2(0.0,1.0);
      vec4 x12=x0.xyxy+C.xxzz; x12.xy-=i1;
      i=mod289(i);
      vec3 p=permute(permute(i.y+vec3(0.0,i1.y,1.0))+i.x+vec3(0.0,i1.x,1.0));
      vec3 m=max(0.5-vec3(dot(x0,x0),dot(x12.xy,x12.xy),dot(x12.zw,x12.zw)),0.0);
      m=m*m; m=m*m;
      vec3 x=2.0*fract(p*0.0243902439)-1.0;
      vec3 h=abs(x)-0.5;
      vec3 ox=floor(x+0.5);
      vec3 a0=x-ox;
      m*=1.79284291400159-0.85373472095314*(a0*a0+h*h);
      vec3 g;
      g.x=a0.x*x0.x+h.x*x0.y;
      g.yz=a0.yz*x12.xz+h.yz*x12.yw;
      return 130.0*dot(m,g);
    }

    void main(){
      vec3 N = normalize(vNormal);
      vec3 V = normalize(cameraPosition - vWorldPosition);
      float fresnel = pow(1.0 - max(dot(N,V), 0.0), 2.0);

      float t = uTime * (0.12 + uIntensity * 0.25);
      float n1 = snoise(vUv*2.0 + vec2( 0.8*t, -0.5*t));
      float n2 = snoise(vUv*3.5 + vec2(-0.3*t,  0.6*t));
      float n  = smoothstep(-0.55, 0.75, 0.6*n1 + 0.4*n2);

      vec3 base  = mix(uBase,   uAccent1, 0.55);
      vec3 tint  = mix(uAccent2,uCore,    0.75);
      vec3 color = mix(base, tint, n * (0.55 + 0.45*uIntensity));

      color += fresnel * uGlow * mix(uAccent1, uCore, 0.9);
      color = mix(color, vec3(1.0), 0.06 + 0.10*uIntensity);

      gl_FragColor = vec4(color, uOpacity);
    }
  `;

  // INDRE – deformert icosa + ekstra detaljer:
  // - to støy-lag for mikrodetalj
  // - enkel “spekk”-glans (rim/spec blend)
  // - lett fargetone fra uniform (kan settes med OKLCH→sRGB)
  const innerVertex = `
    precision highp float;
    varying vec3 vNormal;
    varying vec3 vPos;
    uniform float uTime;
    uniform float uDeform;
    // enkel 3D noise (value)
    float hash(vec3 p){ return fract(sin(dot(p, vec3(127.1,311.7, 74.7)))*43758.5453123); }
    float noise(vec3 p){
      vec3 i=floor(p); vec3 f=fract(p);
      float n= mix(mix(mix( hash(i+vec3(0,0,0)), hash(i+vec3(1,0,0)), f.x),
                       mix( hash(i+vec3(0,1,0)), hash(i+vec3(1,1,0)), f.x), f.y),
                   mix(mix( hash(i+vec3(0,0,1)), hash(i+vec3(1,0,1)), f.x),
                       mix( hash(i+vec3(0,1,1)), hash(i+vec3(1,1,1)), f.x), f.y), f.z);
      return n;
    }
    void main(){
      vNormal = normal;
      vec3 pos = position;

      // to lag støy for mer detalj
      float d1 = noise(normalize(position)*2.6 + vec3(uTime*0.65, 0.0, -uTime*0.44));
      float d2 = noise(normalize(position)*5.0 + vec3(-uTime*0.35, uTime*0.55, 0.2));
      float d = (d1*0.6 + d2*0.4);

      float k = 1.0 + (d - 0.5) * 2.0 * uDeform;
      pos *= k;

      vPos = pos;
      gl_Position = projectionMatrix * modelViewMatrix * vec4(pos,1.0);
    }
  `;
  const innerFragment = `
    precision highp float;
    varying vec3 vNormal;
    varying vec3 vPos;
    uniform float uTime;
    uniform float uOpacity;
    uniform vec3 uColor;
    void main(){
      vec3 N = normalize(vNormal);
      vec3 L1 = normalize(vec3( 0.7, 0.9, 0.4));
      vec3 L2 = normalize(vec3(-0.6, 0.4, 0.7));
      float diff  = clamp(dot(N,L1)*0.5+0.5, 0.0, 1.0);
      float diff2 = clamp(dot(N,L2)*0.5+0.5, 0.0, 1.0);

      // rim/spec miks for litt “krystall”-glans
      vec3 V = normalize(vec3(0.0,0.0,1.0));
      float rim = pow(1.0 - max(dot(N,V),0.0), 2.0);
      float spec = pow(max(dot(normalize(L1+V), N), 0.0), 16.0);

      vec3 base = uColor * (0.75 + 0.25*diff) * (0.7 + 0.3*diff2);
      base += rim * 0.15;
      base += spec * 0.25;

      gl_FragColor = vec4(base, uOpacity);
    }
  `;

  /* ------------------ Halo ------------------ */

  const createHaloTexture = () => {
    const size = 256;
    const c = document.createElement('canvas');
    c.width = c.height = size;
    const ctx = c.getContext('2d')!;
    const g = ctx.createRadialGradient(size/2, size/2, size*0.1, size/2, size/2, size*0.6);
    g.addColorStop(0.0, 'rgba(255,255,255,0.55)');
    g.addColorStop(0.6, 'rgba(220,230,255,0.18)');
    g.addColorStop(1.0, 'rgba(220,230,255,0.00)');
    ctx.fillStyle = g;
    ctx.fillRect(0,0,size,size);
    return c;
  };

  /* ------------------ Partikler ------------------ */

  const fillSpherePoints = (THREE: typeof import('three'), count: number, radius: number) => {
    const pos = new Float32Array(count * 3);
    for (let i=0;i<count;i++){
      let x=0,y=0,z=0;
      do {
        x = (Math.random()*2-1);
        y = (Math.random()*2-1);
        z = (Math.random()*2-1);
      } while (x*x+y*y+z*z > 1);
      const r = radius * Math.cbrt(Math.random());
      pos[i*3+0] = x * r;
      pos[i*3+1] = y * r;
      pos[i*3+2] = z * r;
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    return geo;
  };

  /* ------------------ Init scene ------------------ */

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
      // Canvas
      canvas = document.createElement('canvas');
      canvas.style.width = `${size}px`;
      canvas.style.height = `${size}px`;
      canvas.width = size;
      canvas.height = size;
      canvas.style.borderRadius = '50%';
      canvas.style.display = 'block';
      container.appendChild(canvas);
      canvasRef.current = canvas;

      // Scene/kamera
      scene = new THREE.Scene();
      sceneRef.current = scene;

      camera = new THREE.PerspectiveCamera(30, 1, 0.1, 100);
      camera.position.set(0, 0, 3.3);
      cameraRef.current = camera;

      const webglContext = getCanvasWebGLContext(canvas);
      if (!webglContext) {
        throw new Error('WebGL unavailable');
      }

      // Renderer + skygger
      renderer = new THREE.WebGLRenderer({
        canvas,
        context: webglContext,
        alpha: true,
        antialias: true,
        powerPreference: 'low-power',
      });
      renderer.shadowMap.enabled = true;
      renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      renderer.setSize(size, size, false);
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      rendererRef.current = renderer;

      // Lys + skygge-mottaker
      const dir = new THREE.DirectionalLight(0xffffff, 0.7);
      dir.position.set(2.5, 3.0, 3.0);
      dir.castShadow = true;
      dir.shadow.mapSize.set(512, 512);
      scene.add(dir);

      const amb = new THREE.AmbientLight(0xffffff, 0.35);
      scene.add(amb);

      const plane = new THREE.PlaneGeometry(6, 6);
      const shadowMat = new THREE.ShadowMaterial({ opacity: 0.22 });
      const receiver = new THREE.Mesh(plane, shadowMat);
      receiver.receiveShadow = true;
      receiver.position.set(0, -1.15, 0);
      receiver.rotation.x = -Math.PI/2;
      scene.add(receiver);

      // Halo
      haloTex = new THREE.CanvasTexture(createHaloTexture());
      haloTex.anisotropy = 4;
      const halo = new THREE.Sprite(new THREE.SpriteMaterial({
        map: haloTex, transparent: true, depthWrite: false
      }));
      halo.scale.set(2.8, 2.8, 1);
      halo.position.set(0, 0, -0.08);
      scene.add(halo);

      // Ytre orb (høyere segmenter for fin Fresnel)
      outerGeo = new THREE.SphereGeometry(0.98, 72, 72);
      outerUniforms = {
        uTime:      { value: 0 },
        uIntensity: { value: 0.2 },
        uGlow:      { value: 0.85 },
        uOpacity:   { value: 0.94 },
        uBase:      { value: new THREE.Vector3(0.70,0.76,1.00) }, // default lys blå
        uAccent1:   { value: new THREE.Vector3(0.86,0.90,1.00) },
        uAccent2:   { value: new THREE.Vector3(0.94,0.86,1.00) },
        uCore:      { value: new THREE.Vector3(1.0,1.0,1.0) },
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

      // Indre kjerne – mer detalj (subdiv 4), deform via to støy-lag
      coreGeo = new THREE.IcosahedronGeometry(0.5, 4);
      innerUniforms = {
        uTime:    { value: 0 },
        uDeform:  { value: 0.22 },
        uOpacity: { value: 0.92 },
        uColor:   { value: new THREE.Vector3(1.0,1.0,1.0) }, // settes per state
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

      // Partikler
      const particleCount = 900;
      const pGeo = fillSpherePoints(THREE, particleCount, 0.9);
      const pMat = new THREE.PointsMaterial({
        size: 0.018, sizeAttenuation: true, transparent: true,
        color: 0xffffff, opacity: 0.65, depthWrite: false
      });
      const points = new THREE.Points(pGeo, pMat);
      particlesRef.current = points;
      scene.add(points);
    } catch {
      setHasRendererFailure(true);
      if (renderer) {
        renderer.dispose();
      }
      if (haloTex) {
        haloTex.dispose();
      }
      if (outerGeo) {
        outerGeo.dispose();
      }
      if (coreGeo) {
        coreGeo.dispose();
      }
      if (canvas && container.contains(canvas)) {
        container.removeChild(canvas);
      }
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

    // Tilstander – setter palett og styrker
  const applyState = (s: OrbAnimationState) => {
      const ou = outerUniformsRef.current;
      const iu = innerUniformsRef.current;
      if (!ou || !iu || !particlesRef.current) return;
      const pm = (particlesRef.current.material as import('three').PointsMaterial);

      switch (s) {
        case 'waiting': {
          // YTRE: lys blå (pastell)
          ou.uBase.value.set(0.70,0.76,1.00);   // ~ #8fa0ff
          ou.uAccent1.value.set(0.86,0.90,1.00);// ~ #dbe1ff
          ou.uAccent2.value.set(0.92,0.88,1.00);// svakt lillaskjær
          ou.uCore.value.set(1.0,1.0,1.0);
          ou.uIntensity.value = 0.18;
          ou.uGlow.value = 0.85;
          ou.uOpacity.value = 0.94;
          baseValuesRef.current.intensity = 0.18;

          // INDRE: mørk blå = BRAND PRIMARY (OKLCH → sRGB)
          const prim = oklchVarToRGB('--primary', 'oklch(0.292 0.132 269.8)');
          iu.uDeform.value = 0.20;
          iu.uOpacity.value = 0.95;
          iu.uColor.value.set(prim.r, prim.g, prim.b);
          baseValuesRef.current.deform = 0.20;

          // Partikler: blålige, litt roligere
          pm.color.setStyle('#cfd6ff');
          pm.opacity = 0.55;
          break;
        }
        case 'listening': {
          // Oransje/gull
          ou.uBase.value.set(1.00,0.78,0.50);
          ou.uAccent1.value.set(1.00,0.88,0.63);
          ou.uAccent2.value.set(1.00,0.92,0.78);
          ou.uCore.value.set(1.0,1.0,1.0);
          ou.uIntensity.value = 0.45;
          ou.uGlow.value = 1.0;
          ou.uOpacity.value = 0.96;
          baseValuesRef.current.intensity = 0.45;

          iu.uDeform.value = 0.24;
          iu.uOpacity.value = 0.94;
          iu.uColor.value.set(1.00,0.90,0.70);
          baseValuesRef.current.deform = 0.24;

          pm.color.setStyle('#ffd27a');
          pm.opacity = 0.72;
          break;
        }
        case 'thinking': {
          // Offwhite orb + svarte partikler
          ou.uBase.value.set(0.96,0.97,1.00);
          ou.uAccent1.value.set(0.98,0.99,1.00);
          ou.uAccent2.value.set(0.93,0.95,1.00);
          ou.uCore.value.set(1.0,1.0,1.0);
          ou.uIntensity.value = 0.58;
          ou.uGlow.value = 0.95;
          ou.uOpacity.value = 0.97;
          baseValuesRef.current.intensity = 0.58;

          iu.uDeform.value = 0.28;
          iu.uOpacity.value = 0.9;
          iu.uColor.value.set(0.98,0.99,1.0);
          baseValuesRef.current.deform = 0.28;

          pm.color.set(0x111111);
          pm.opacity = 0.85;
          break;
        }
        case 'responding': {
          // Grønn
          ou.uBase.value.set(0.55,0.95,0.70);
          ou.uAccent1.value.set(0.70,1.00,0.80);
          ou.uAccent2.value.set(0.82,1.00,0.90);
          ou.uCore.value.set(1.0,1.0,1.0);
          ou.uIntensity.value = 0.78;
          ou.uGlow.value = 1.15;
          ou.uOpacity.value = 0.96;
          baseValuesRef.current.intensity = 0.78;

          iu.uDeform.value = 0.32;
          iu.uOpacity.value = 0.95;
          iu.uColor.value.set(0.85,1.0,0.9);
          baseValuesRef.current.deform = 0.32;

          pm.color.setStyle('#b6ffd6');
          pm.opacity = 0.78;
          break;
        }
      }
    };
    applyState(state);

    // Animasjon
    const clock = new THREE.Clock();
      const animate = () => {
      const dt = clock.getDelta();
      outerUniforms.uTime.value += dt;
      innerUniforms.uTime.value += dt;

      const t = outerUniforms.uTime.value;
        const st = stateRef.current; // latest state

      // Dynamic parameters per state
      const baseSpin =
        st === 'thinking'   ? 1.2 :
        st === 'responding' ? 0.85 :
        st === 'listening'  ? 0.55 : 0.35;

      const spinX =
        st === 'thinking'   ? 1.05 :
        st === 'responding' ? 0.65 :
        st === 'listening'  ? 0.40 : 0.18;

      // Breathing / pulses
      let scaleMod = 0;
      if (st === 'waiting') {
        scaleMod = Math.sin(t * 1.1) * 0.025 + Math.sin(t * 0.37) * 0.01; // layered gentle
      } else if (st === 'listening') {
        // double beat (heartbeat style)
        const beat1 = Math.max(0, Math.sin(t * 3.2));
        const beat2 = Math.max(0, Math.sin((t * 3.2) + 1.2));
        scaleMod = (beat1 * 0.05 + beat2 * 0.035);
      } else if (st === 'thinking') {
        scaleMod = Math.sin(t * 2.4) * 0.018; // subtle fast micro-churn
      } else if (st === 'responding') {
        scaleMod = Math.sin(t * 4.0) * 0.035 + Math.sin(t * 1.3) * 0.025; // energetic mix
      }

      if (orbMeshRef.current) {
        const s = 1.0 + scaleMod;
        orbMeshRef.current.scale.setScalar(s);
        orbMeshRef.current.rotation.y += baseSpin * dt;
        orbMeshRef.current.rotation.x += spinX * dt;
        // subtle Z precession for thinking/responding
        if (st === 'thinking' || st === 'responding') {
          orbMeshRef.current.rotation.z += (st === 'thinking' ? 0.35 : 0.55) * dt;
        }
      }

      // Indre kjerne – mer liv: multi-akse + mikro-wobble i "waiting"
      if (coreMeshRef.current) {
        const wobble = st === 'waiting' ? Math.sin(t * 1.6) * 0.08 : st === 'thinking' ? Math.sin(t * 3.0) * 0.05 : 0.03;
        coreMeshRef.current.rotation.y -= (baseSpin * 0.66) * dt;
        coreMeshRef.current.rotation.x += (spinX * 0.58) * dt + wobble * dt;
        coreMeshRef.current.rotation.z += (st === 'responding' ? 0.55 : 0.28) * dt;
        // Crystal micro pulse
        let coreScale = 1.0 + Math.sin(t * 1.25) * 0.012;
        if (st === 'responding') coreScale += Math.sin(t * 5.2) * 0.018;
        if (st === 'listening') coreScale += Math.max(0, Math.sin(t * 3.2)) * 0.02;
        coreMeshRef.current.scale.setScalar(coreScale);
        // Dynamic deform modulation for morphing
        if (innerUniformsRef.current) {
          const baseDef = baseValuesRef.current.deform;
            innerUniformsRef.current.uDeform.value =
              baseDef + (
                st === 'responding' ? (Math.sin(t * 4.5) * 0.08 + Math.sin(t * 2.1) * 0.04) :
                st === 'thinking'   ? Math.sin(t * 2.2) * 0.05 :
                st === 'listening'  ? Math.sin(t * 3.2) * 0.035 :
                Math.sin(t * 1.4) * 0.02
              );
        }
      }

      // Partikler – swirl + radial pust
      if (particlesRef.current) {
  const geo = particlesRef.current.geometry as import('three').BufferGeometry;
  const pos = geo.getAttribute('position') as import('three').BufferAttribute;
        const count = pos.count;
        const swirl = (st === 'thinking' ? 1.15 :
                       st === 'responding' ? 0.95 :
                       st === 'listening' ? 0.62 : 0.38) * dt;

        for (let i=0;i<count;i++){
          const ix = i*3;
          const x = pos.array[ix+0];
          const z = pos.array[ix+2];
          const r = Math.hypot(x,z) + 1e-6;
          const ang = Math.atan2(z, x) + swirl * (0.38 + (i%5)*0.01);
          pos.array[ix+0] = Math.cos(ang) * r;
          pos.array[ix+2] = Math.sin(ang) * r;

          const y = pos.array[ix+1];
          const rr = Math.sqrt(x*x + y*y + z*z);
          const push = Math.sin(t*1.25 + i*0.011) * 0.0009 *
                       (st==='responding'?2.3: st==='thinking'?1.8: st==='listening'?1.5:1.0);
          const f = (rr>0.0001) ? (rr+push)/rr : 1.0;
          pos.array[ix+0] *= f; pos.array[ix+1] *= f; pos.array[ix+2] *= f;
        }
        pos.needsUpdate = true;
      }

      // Subtle intensity modulation (breathing aura)
      if (outerUniformsRef.current) {
        const baseInt = baseValuesRef.current.intensity;
        const mod =
          st === 'waiting'   ? Math.sin(t * 0.8) * 0.04 :
          st === 'listening' ? Math.sin(t * 3.2) * 0.06 :
          st === 'thinking'  ? Math.sin(t * 2.0) * 0.05 :
                               Math.sin(t * 4.0) * 0.08; // responding
        outerUniformsRef.current.uIntensity.value = baseInt + mod;
      }

      renderer.render(scene, camera);
      animationIdRef.current = requestAnimationFrame(animate);
    };
    animate();

    // Rydding
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

  /* ------------------ State-endringer + auto-overgang ------------------ */

  useEffect(() => {
    const ou = outerUniformsRef.current;
    const iu = innerUniformsRef.current;
    if (!ou || !iu) return;

    // Sett palett ved state-endring (samme som i init → applyState)
    const setWaiting = () => {
      ou.uBase.value.set(0.70,0.76,1.00);
      ou.uAccent1.value.set(0.86,0.90,1.00);
      ou.uAccent2.value.set(0.92,0.88,1.00);
      ou.uCore.value.set(1.0,1.0,1.0);
      ou.uIntensity.value = 0.18; ou.uGlow.value = 0.85; ou.uOpacity.value = 0.94;

      const prim = oklchVarToRGB('--primary','oklch(0.292 0.132 269.8)');
      iu.uDeform.value = 0.20; iu.uOpacity.value = 0.95; iu.uColor.value.set(prim.r,prim.g,prim.b);

      if (particlesRef.current) {
  const pm = particlesRef.current.material as import('three').PointsMaterial;
        pm.color.setStyle('#cfd6ff'); pm.opacity = 0.55;
      }
    };
    const setListening = () => {
      ou.uBase.value.set(1.00,0.78,0.50);
      ou.uAccent1.value.set(1.00,0.88,0.63);
      ou.uAccent2.value.set(1.00,0.92,0.78);
      ou.uCore.value.set(1.0,1.0,1.0);
      ou.uIntensity.value = 0.45; ou.uGlow.value = 1.0; ou.uOpacity.value = 0.96;

      iu.uDeform.value = 0.24; iu.uOpacity.value = 0.94; iu.uColor.value.set(1.00,0.90,0.70);

      if (particlesRef.current) {
  const pm = particlesRef.current.material as import('three').PointsMaterial;
        pm.color.setStyle('#ffd27a'); pm.opacity = 0.72;
      }
    };
    const setThinking = () => {
      ou.uBase.value.set(0.96,0.97,1.00);
      ou.uAccent1.value.set(0.98,0.99,1.00);
      ou.uAccent2.value.set(0.93,0.95,1.00);
      ou.uCore.value.set(1.0,1.0,1.0);
      ou.uIntensity.value = 0.58; ou.uGlow.value = 0.95; ou.uOpacity.value = 0.97;

      iu.uDeform.value = 0.28; iu.uOpacity.value = 0.9; iu.uColor.value.set(0.98,0.99,1.0);

      if (particlesRef.current) {
  const pm = particlesRef.current.material as import('three').PointsMaterial;
        pm.color.set(0x111111); pm.opacity = 0.85;
      }
    };
    const setResponding = () => {
      ou.uBase.value.set(0.55,0.95,0.70);
      ou.uAccent1.value.set(0.70,1.00,0.80);
      ou.uAccent2.value.set(0.82,1.00,0.90);
      ou.uCore.value.set(1.0,1.0,1.0);
      ou.uIntensity.value = 0.78; ou.uGlow.value = 1.15; ou.uOpacity.value = 0.96;

      iu.uDeform.value = 0.32; iu.uOpacity.value = 0.95; iu.uColor.value.set(0.85,1.0,0.9);

      if (particlesRef.current) {
  const pm = particlesRef.current.material as import('three').PointsMaterial;
        pm.color.setStyle('#b6ffd6'); pm.opacity = 0.78;
      }
    };

    if (state === 'waiting') setWaiting();
    if (state === 'listening') setListening();
    if (state === 'thinking') setThinking();
    if (state === 'responding') setResponding();

    // Auto-overganger som før
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

  // Fallback
  if (!isReady || hasRendererFailure) {
    return (
      <div
        className={`three-orb-fallback ${className}`}
        style={{
          width: size, height: size, borderRadius: '50%',
          background:
            'radial-gradient(60% 60% at 50% 50%, rgba(255,255,255,0.95) 0%, rgba(207,214,255,0.75) 55%, rgba(143,160,255,0.60) 100%)',
          boxShadow: '0 10px 30px rgba(0,0,0,0.18), 0 0 30px rgba(180,190,255,0.45)',
          animation: 'orb-breathe 3.2s ease-in-out infinite'
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

export default ThreeJSOrb;
