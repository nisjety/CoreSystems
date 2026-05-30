/**
 * GLSL shader sources for the ThreeJS Orb.
 * Outer: Fresnel + simplex-noise fluid glow.
 * Inner: Deformed icosphere with dual-noise and specular rim.
 */

// ── Outer sphere ──────────────────────────────────────────────────────────

export const outerVertex = `
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

export const outerFragment = `
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

// ── Inner icosphere ───────────────────────────────────────────────────────

export const innerVertex = `
  precision highp float;
  varying vec3 vNormal;
  varying vec3 vPos;
  uniform float uTime;
  uniform float uDeform;
  // simple 3D value noise
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

    // dual-layer noise for detail
    float d1 = noise(normalize(position)*2.6 + vec3(uTime*0.65, 0.0, -uTime*0.44));
    float d2 = noise(normalize(position)*5.0 + vec3(-uTime*0.35, uTime*0.55, 0.2));
    float d = (d1*0.6 + d2*0.4);

    float k = 1.0 + (d - 0.5) * 2.0 * uDeform;
    pos *= k;

    vPos = pos;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(pos,1.0);
  }
`;

export const innerFragment = `
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

    // rim/spec blend for crystal sheen
    vec3 V = normalize(vec3(0.0,0.0,1.0));
    float rim = pow(1.0 - max(dot(N,V),0.0), 2.0);
    float spec = pow(max(dot(normalize(L1+V), N), 0.0), 16.0);

    vec3 base = uColor * (0.75 + 0.25*diff) * (0.7 + 0.3*diff2);
    base += rim * 0.15;
    base += spec * 0.25;

    gl_FragColor = vec4(base, uOpacity);
  }
`;
