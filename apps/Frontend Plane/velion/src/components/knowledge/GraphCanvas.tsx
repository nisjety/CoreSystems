'use client';

import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { GraphViewerSnapshot } from '@/types/data-plane/graph_v1';

interface GraphCanvasProps {
  snapshot: GraphViewerSnapshot;
  selectedId: string | null;
  onSelect: (entityId: string | null) => void;
  /** Filter chips: when non-empty, only show entities of these types. */
  visibleTypes: ReadonlySet<string>;
}

interface SimNode {
  id: string;
  label: string;
  type: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
  fixed: boolean;
}

interface SimEdge {
  source: string;
  target: string;
  type: string;
}

const NODE_RADIUS = 8;
const REPULSION = 6000;
const SPRING_K = 0.02;
const SPRING_LEN = 90;
const DAMPING = 0.82;
const CENTER_PULL = 0.002;
const TYPE_COLORS: Record<string, string> = {
  person: '#7C3AED',
  organization: '#DC2626',
  product: '#0EA5E9',
  location: '#059669',
  concept: '#F59E0B',
  default: '#6B7280',
};

function colorFor(type: string): string {
  return TYPE_COLORS[type.toLowerCase()] ?? TYPE_COLORS.default;
}

/**
 * Wave 11.C-a — Reflect "Map"-style force-directed graph (Mobbin
 * `0e1be504-3f40-4cd2-8968-091212a155e5`).
 *
 * Renders entities as colored nodes, relationships as edges. Click a
 * node to open the entity drawer (handled by parent). Drag to pan,
 * scroll to zoom. No external deps — a small 2D physics loop runs in
 * a useEffect with requestAnimationFrame; we stop simulating once
 * the graph settles.
 */
export function GraphCanvas({
  snapshot,
  selectedId,
  onSelect,
  visibleTypes,
}: GraphCanvasProps): ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const nodesRef = useRef<Map<string, SimNode>>(new Map());
  const edgesRef = useRef<SimEdge[]>([]);
  const rafRef = useRef<number | null>(null);
  const energyRef = useRef<number>(Infinity);
  const [transform, setTransform] = useState<{ x: number; y: number; k: number }>({
    x: 0,
    y: 0,
    k: 1,
  });
  const [dragging, setDragging] = useState<'pan' | string | null>(null);
  const dragStartRef = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);

  // Sync sim state with incoming snapshot.
  useEffect(() => {
    const w = canvasRef.current?.parentElement?.clientWidth ?? 800;
    const h = canvasRef.current?.parentElement?.clientHeight ?? 600;
    const existing = nodesRef.current;
    const next = new Map<string, SimNode>();
    for (const entity of snapshot.entities) {
      if (visibleTypes.size > 0 && !visibleTypes.has(entity.type)) continue;
      const prior = existing.get(entity.entity_id);
      next.set(entity.entity_id, {
        id: entity.entity_id,
        label: entity.text,
        type: entity.type,
        x: prior?.x ?? Math.random() * w,
        y: prior?.y ?? Math.random() * h,
        vx: prior?.vx ?? 0,
        vy: prior?.vy ?? 0,
        fixed: prior?.fixed ?? false,
      });
    }
    nodesRef.current = next;
    edgesRef.current = snapshot.relationships
      .filter((r) => next.has(r.entity_a_id) && next.has(r.entity_b_id))
      .map((r) => ({
        source: r.entity_a_id,
        target: r.entity_b_id,
        type: r.relation_type,
      }));
    energyRef.current = Infinity; // wake the sim
  }, [snapshot, visibleTypes]);

  // Physics + render loop.
  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;

    const resize = (): void => {
      const w = container.clientWidth;
      const h = container.clientHeight;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(container);

    const step = (): void => {
      const nodes = Array.from(nodesRef.current.values());
      const edges = edgesRef.current;
      const cx = container.clientWidth / 2;
      const cy = container.clientHeight / 2;

      let energy = 0;

      // Repulsion (Coulomb)
      for (let i = 0; i < nodes.length; i++) {
        const a = nodes[i];
        if (a.fixed) continue;
        for (let j = i + 1; j < nodes.length; j++) {
          const b = nodes[j];
          const dx = a.x - b.x;
          const dy = a.y - b.y;
          const dist2 = dx * dx + dy * dy + 0.01;
          const force = REPULSION / dist2;
          const dist = Math.sqrt(dist2);
          const fx = (dx / dist) * force;
          const fy = (dy / dist) * force;
          a.vx += fx;
          a.vy += fy;
          if (!b.fixed) {
            b.vx -= fx;
            b.vy -= fy;
          }
        }
      }

      // Springs (edges)
      for (const edge of edges) {
        const a = nodesRef.current.get(edge.source);
        const b = nodesRef.current.get(edge.target);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const displacement = dist - SPRING_LEN;
        const force = SPRING_K * displacement;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        if (!a.fixed) {
          a.vx += fx;
          a.vy += fy;
        }
        if (!b.fixed) {
          b.vx -= fx;
          b.vy -= fy;
        }
      }

      // Centering + damping + integration
      for (const node of nodes) {
        if (node.fixed) continue;
        node.vx += (cx - node.x) * CENTER_PULL;
        node.vy += (cy - node.y) * CENTER_PULL;
        node.vx *= DAMPING;
        node.vy *= DAMPING;
        node.x += node.vx;
        node.y += node.vy;
        energy += Math.abs(node.vx) + Math.abs(node.vy);
      }
      energyRef.current = energy;

      // Render
      ctx.clearRect(0, 0, container.clientWidth, container.clientHeight);
      ctx.save();
      ctx.translate(transform.x, transform.y);
      ctx.scale(transform.k, transform.k);

      // Edges
      ctx.strokeStyle = '#D1D5DB';
      ctx.lineWidth = 1 / transform.k;
      for (const edge of edges) {
        const a = nodesRef.current.get(edge.source);
        const b = nodesRef.current.get(edge.target);
        if (!a || !b) continue;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      }

      // Nodes
      for (const node of nodes) {
        const isSelected = node.id === selectedId;
        ctx.beginPath();
        ctx.arc(node.x, node.y, NODE_RADIUS + (isSelected ? 3 : 0), 0, Math.PI * 2);
        ctx.fillStyle = colorFor(node.type);
        ctx.fill();
        if (isSelected) {
          ctx.lineWidth = 2 / transform.k;
          ctx.strokeStyle = '#111111';
          ctx.stroke();
        }
        // Label
        ctx.fillStyle = '#111827';
        ctx.font = `${11 / transform.k}px ui-sans-serif, system-ui`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        const label =
          node.label.length > 32 ? `${node.label.slice(0, 30)}…` : node.label;
        ctx.fillText(label, node.x, node.y + NODE_RADIUS + 4 / transform.k);
      }

      ctx.restore();

      if (energy > 1) {
        rafRef.current = requestAnimationFrame(step);
      } else {
        rafRef.current = null;
      }
    };

    rafRef.current = requestAnimationFrame(step);

    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      ro.disconnect();
    };
  }, [transform, selectedId]);

  const screenToGraph = (clientX: number, clientY: number): { x: number; y: number } => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const sx = clientX - rect.left - transform.x;
    const sy = clientY - rect.top - transform.y;
    return { x: sx / transform.k, y: sy / transform.k };
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLCanvasElement>): void => {
    const { x: gx, y: gy } = screenToGraph(event.clientX, event.clientY);
    let hit: string | null = null;
    for (const node of nodesRef.current.values()) {
      const dx = node.x - gx;
      const dy = node.y - gy;
      if (dx * dx + dy * dy <= (NODE_RADIUS + 4) ** 2) {
        hit = node.id;
        break;
      }
    }
    if (hit) {
      onSelect(hit);
      setDragging(hit);
      const node = nodesRef.current.get(hit)!;
      node.fixed = true;
      energyRef.current = Infinity;
      if (rafRef.current === null) {
        rafRef.current = requestAnimationFrame(() => {});
      }
      return;
    }
    setDragging('pan');
    dragStartRef.current = {
      x: event.clientX,
      y: event.clientY,
      tx: transform.x,
      ty: transform.y,
    };
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLCanvasElement>): void => {
    if (!dragging) return;
    if (dragging === 'pan' && dragStartRef.current) {
      const dx = event.clientX - dragStartRef.current.x;
      const dy = event.clientY - dragStartRef.current.y;
      setTransform({
        x: dragStartRef.current.tx + dx,
        y: dragStartRef.current.ty + dy,
        k: transform.k,
      });
      return;
    }
    const node = nodesRef.current.get(dragging);
    if (!node) return;
    const { x: gx, y: gy } = screenToGraph(event.clientX, event.clientY);
    node.x = gx;
    node.y = gy;
    node.vx = 0;
    node.vy = 0;
    energyRef.current = Infinity;
  };

  const handlePointerUp = (): void => {
    if (typeof dragging === 'string' && dragging !== 'pan') {
      const node = nodesRef.current.get(dragging);
      if (node) node.fixed = false;
    }
    setDragging(null);
    dragStartRef.current = null;
  };

  const handleWheel = (event: React.WheelEvent<HTMLCanvasElement>): void => {
    const factor = event.deltaY < 0 ? 1.1 : 1 / 1.1;
    const next = Math.max(0.25, Math.min(3, transform.k * factor));
    setTransform((prev) => ({ ...prev, k: next }));
  };

  return (
    <div
      ref={containerRef}
      className="relative h-full w-full overflow-hidden bg-white"
    >
      <canvas
        ref={canvasRef}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerLeave={handlePointerUp}
        onWheel={handleWheel}
        className={dragging === 'pan' ? 'cursor-grabbing' : 'cursor-grab'}
      />
      {snapshot.entities.length === 0 ? (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-center">
          <div className="max-w-[40ch]">
            <p className="text-[14px] font-medium text-[#111827]">No graph yet</p>
            <p className="mt-1 text-[12px] text-[#6B7280]">
              The graph builds as documents flow through the extraction pipeline.
              Add some Files / Website crawls and check back in a minute.
            </p>
          </div>
        </div>
      ) : null}
    </div>
  );
}
