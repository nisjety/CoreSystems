"use client";

import * as React from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import * as THREE from "three";

function AccentFlowLine() {
  const meshRef = React.useRef<THREE.Mesh>(null);
  const scrollRef = React.useRef(0);

  React.useEffect(() => {
    const onScroll = () => {
      const doc = document.documentElement;
      const max = Math.max(1, doc.scrollHeight - window.innerHeight);
      scrollRef.current = window.scrollY / max;
    };

    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });

    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  const curve = React.useMemo(() => {
    return new THREE.CatmullRomCurve3([
      new THREE.Vector3(-2.2, -0.9, -0.2),
      new THREE.Vector3(-1.2, -0.2, -0.1),
      new THREE.Vector3(0.3, 0.15, 0),
      new THREE.Vector3(1.6, 0.4, 0.1),
    ]);
  }, []);

  const geometry = React.useMemo(() => {
    return new THREE.TubeGeometry(curve, 180, 0.02, 12, false);
  }, [curve]);

  React.useEffect(() => {
    return () => geometry.dispose();
  }, [geometry]);

  useFrame((state) => {
    if (!meshRef.current) return;

    const t = state.clock.getElapsedTime();
    const s = scrollRef.current;

    meshRef.current.position.y = Math.sin(t * 0.4) * 0.015 + s * 0.08;
    meshRef.current.rotation.z = -0.04 + s * 0.04;
  });

  return (
    <mesh ref={meshRef} geometry={geometry}>
      <meshBasicMaterial
        color="#FF5E5B"
        transparent
        opacity={0.55}
        toneMapped={false}
      />
    </mesh>
  );
}

export default function HeroNeonFlow3D() {
  return (
    <div className="pointer-events-none absolute inset-0 z-20">
      <Canvas
        className="h-full w-full"
        camera={{ position: [0, 0, 5], fov: 40 }}
        gl={{
          alpha: true,
          antialias: true,
          powerPreference: "high-performance",
        }}
        onCreated={({ gl }) => {
          gl.toneMapping = THREE.NoToneMapping;
        }}
      >
        <AccentFlowLine />
      </Canvas>
    </div>
  );
}
