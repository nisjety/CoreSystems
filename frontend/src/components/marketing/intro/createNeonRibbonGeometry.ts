import * as THREE from 'three';

type Options = {
  segments: number;
  widthStart: number;
  widthEnd: number;
};

/**
 * Lager en “ribbon” (flat stripe) langs en kurve med taper.
 * Dette gir en skarp, kontrollert “flow”-linje uten å bli en tykk tube.
 */
export function createNeonRibbonGeometry(curve: THREE.Curve<THREE.Vector3>, opts: Options) {
  const { segments, widthStart, widthEnd } = opts;

  const positions: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];

  const tmpTangent = new THREE.Vector3();
  const tmpNormal = new THREE.Vector3();
  const tmpBinormal = new THREE.Vector3();
  const up = new THREE.Vector3(0, 1, 0);

  // Bygger to punkter per segment (venstre/høyre side av stripen)
  for (let i = 0; i <= segments; i++) {
    const t = i / segments;
    const p = curve.getPointAt(t);

    curve.getTangentAt(t, tmpTangent).normalize();

    // Stabil normal: kryss tangent med “up” – faller back hvis parallell
    tmpNormal.copy(up).cross(tmpTangent);
    if (tmpNormal.lengthSq() < 1e-6) tmpNormal.set(1, 0, 0).cross(tmpTangent);
    tmpNormal.normalize();

    tmpBinormal.copy(tmpTangent).cross(tmpNormal).normalize();

    // Taper: bredde går fra start til slutt (kryptert “flow”)
    const w = THREE.MathUtils.lerp(widthStart, widthEnd, smoothstep(0.0, 1.0, t));
    const half = w * 0.5;

    const left = p.clone().add(tmpNormal.clone().multiplyScalar(-half));
    const right = p.clone().add(tmpNormal.clone().multiplyScalar(half));

    // pos
    positions.push(left.x, left.y, left.z);
    positions.push(right.x, right.y, right.z);

    // uv (u = langs linja, v = 0/1 på bredden)
    uvs.push(t, 0);
    uvs.push(t, 1);

    // indeks (to tri per segment)
    if (i < segments) {
      const a = i * 2;
      const b = i * 2 + 1;
      const c = i * 2 + 2;
      const d = i * 2 + 3;

      indices.push(a, b, c);
      indices.push(b, d, c);
    }
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setIndex(indices);
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  geometry.computeVertexNormals();

  return geometry;
}

function smoothstep(edge0: number, edge1: number, x: number) {
  const t = THREE.MathUtils.clamp((x - edge0) / (edge1 - edge0), 0, 1);
  return t * t * (3 - 2 * t);
}
