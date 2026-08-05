import type { CSSProperties, ReactNode } from 'react'
import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion'

const CREAM = '#F4EFE5'
const PAPER = '#FFFDF8'
const TEXT = '#111111'
const MUTED = '#777169'
const LINE = '#DDD7CC'
const CORAL = '#FF2E63'
const GREEN = '#39B980'
const BLUE = '#4D8DFF'
const GOLD = '#F5D77F'

const sourceCards = [
  {
    label: 'triodelab.no',
    title: 'Website indexed',
    meta: '24 pages',
    color: CORAL,
    x: 255,
    y: 235,
    delay: 4,
    rotate: -7,
  },
  {
    label: 'Microsoft 365',
    title: 'Workspace connected',
    meta: 'Teams, SharePoint, Drive',
    color: BLUE,
    x: 1185,
    y: 180,
    delay: 22,
    rotate: 5,
  },
  {
    label: 'Brreg',
    title: 'Company context found',
    meta: 'Size and industry',
    color: GOLD,
    x: 245,
    y: 665,
    delay: 42,
    rotate: 6,
  },
  {
    label: 'Knowledge graph',
    title: 'Sources linked',
    meta: 'People, pages, documents',
    color: GREEN,
    x: 1230,
    y: 670,
    delay: 62,
    rotate: -5,
  },
]

const graphNodes = [
  { id: 'org', x: 50, y: 52, r: 8.8, color: GOLD, label: 'Org' },
  { id: 'web', x: 28, y: 35, r: 5.2, color: CORAL, label: 'Website' },
  { id: 'm365', x: 69, y: 34, r: 5.8, color: BLUE, label: 'M365' },
  { id: 'docs', x: 78, y: 60, r: 4.8, color: GREEN, label: 'Docs' },
  { id: 'team', x: 35, y: 72, r: 4.8, color: BLUE, label: 'Team' },
  { id: 'agent', x: 18, y: 56, r: 4.8, color: GREEN, label: 'Agent' },
]

const graphEdges = [
  ['org', 'web'],
  ['org', 'm365'],
  ['org', 'docs'],
  ['org', 'team'],
  ['org', 'agent'],
  ['web', 'agent'],
  ['m365', 'docs'],
]

export function ProductReveal() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const camera = spring({
    frame,
    fps,
    config: { damping: 150, stiffness: 60, mass: 1.15 },
    durationInFrames: 165,
  })
  const finalFocus = progress(frame, 142, 192)
  const worldX = interpolate(camera, [0, 1], [-76, 20], {
    extrapolateRight: 'clamp',
  })
  const worldY = interpolate(camera, [0, 1], [18, -10], {
    extrapolateRight: 'clamp',
  })
  const worldScale = interpolate(camera, [0, 1], [1.075, 0.985], {
    extrapolateRight: 'clamp',
  })
  const worldOpacity = interpolate(finalFocus, [0, 1], [1, 0.64])

  return (
    <AbsoluteFill
      style={{
        background: CREAM,
        color: TEXT,
        fontFamily:
          'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
        overflow: 'hidden',
      }}
    >
      <StudioBackdrop frame={frame} />
      <Noise />
      <div
        style={{
          position: 'absolute',
          inset: 0,
          opacity: worldOpacity,
          transform: `translate3d(${worldX}px, ${worldY}px, 0) scale(${worldScale})`,
          transformOrigin: '50% 52%',
        }}
      >
        <CurvedMotionPath frame={frame} />
        <FloatingObjects frame={frame} />
        {sourceCards.map((card, index) => (
          <SourceCard key={card.label} frame={frame} index={index} {...card} />
        ))}
        <KnowledgeGraph frame={frame} />
        <MetricCluster frame={frame} />
      </div>
      <FinalReplyCard frame={frame} focus={finalFocus} />
      <Wordmark frame={frame} />
      <LensVignette />
    </AbsoluteFill>
  )
}

function StudioBackdrop({ frame }: { frame: number }) {
  const glowX = interpolate(frame, [0, 180], [44, 56], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  return (
    <>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          background:
            `radial-gradient(ellipse at ${glowX}% 35%, rgba(255,255,255,0.78), rgba(255,255,255,0) 34%), ` +
            'linear-gradient(180deg, rgba(255,255,255,0.58), rgba(244,239,229,0.86) 56%, rgba(231,224,211,0.78))',
        }}
      />
      <div
        style={{
          position: 'absolute',
          left: 220,
          right: 220,
          bottom: 80,
          height: 250,
          borderRadius: '50%',
          background:
            'radial-gradient(ellipse, rgba(17,17,17,0.16), rgba(17,17,17,0.06) 42%, rgba(17,17,17,0) 72%)',
          filter: 'blur(18px)',
          transform: 'scaleX(1.18)',
        }}
      />
      <div
        style={{
          position: 'absolute',
          left: -90,
          right: -90,
          bottom: -230,
          height: 560,
          borderRadius: '50% 50% 0 0',
          borderTop: '1px solid rgba(17,17,17,0.08)',
          background:
            'linear-gradient(180deg, rgba(255,255,255,0.30), rgba(255,255,255,0))',
        }}
      />
      <div
        style={{
          position: 'absolute',
          inset: 0,
          opacity: 0.18,
          backgroundImage:
            'linear-gradient(90deg, rgba(17,17,17,0.05) 1px, transparent 1px), linear-gradient(0deg, rgba(17,17,17,0.035) 1px, transparent 1px)',
          backgroundSize: '360px 360px',
          maskImage:
            'radial-gradient(ellipse at 50% 52%, black 0%, black 45%, transparent 76%)',
        }}
      />
    </>
  )
}

function CurvedMotionPath({ frame }: { frame: number }) {
  const draw = progress(frame, 5, 86)
  return (
    <svg
      viewBox="0 0 1920 1080"
      style={{
        position: 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        opacity: interpolate(frame, [0, 25, 180, 230], [0, 0.55, 0.44, 0], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
        }),
      }}
    >
      <path
        d="M 228 650 C 455 368, 676 282, 1015 470 S 1438 759, 1664 389"
        fill="none"
        stroke="rgba(17,17,17,0.25)"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray="1800"
        strokeDashoffset={1800 - draw * 1800}
      />
      <path
        d="M 228 650 C 455 368, 676 282, 1015 470 S 1438 759, 1664 389"
        fill="none"
        stroke={CORAL}
        strokeWidth="3"
        strokeLinecap="round"
        strokeDasharray="1800"
        strokeDashoffset={1800 - draw * 1800}
        opacity={0.18}
        filter="blur(5px)"
      />
    </svg>
  )
}

function FloatingObjects({ frame }: { frame: number }) {
  return (
    <>
      <DataPrimitive
        frame={frame}
        kind="capsule"
        x={106}
        y={155}
        delay={8}
        rotate={-24}
        color="#ECE8DF"
      />
      <DataPrimitive
        frame={frame}
        kind="tile"
        x={1595}
        y={166}
        delay={18}
        rotate={18}
        color="#FFFFFF"
      />
      <DataPrimitive
        frame={frame}
        kind="ring"
        x={146}
        y={792}
        delay={38}
        rotate={12}
        color="#F0D1D9"
      />
      <DataPrimitive
        frame={frame}
        kind="wedge"
        x={1650}
        y={762}
        delay={52}
        rotate={-16}
        color="#D8E7FF"
      />
    </>
  )
}

function DataPrimitive({
  frame,
  kind,
  x,
  y,
  delay,
  rotate,
  color,
}: {
  frame: number
  kind: 'capsule' | 'tile' | 'ring' | 'wedge'
  x: number
  y: number
  delay: number
  rotate: number
  color: string
}) {
  const enter = progress(frame, delay, delay + 42)
  const driftY = Math.sin((frame + delay * 2) / 28) * 10
  const driftX = Math.cos((frame + delay) / 36) * 8
  const style: CSSProperties = {
    position: 'absolute',
    left: x,
    top: y,
    opacity: enter * 0.92,
    transform: `translate3d(${(1 - enter) * 26 + driftX}px, ${(1 - enter) * 38 + driftY}px, 0) rotate(${rotate + Math.sin(frame / 65) * 4}deg) scale(${0.78 + enter * 0.22})`,
    filter: `blur(${(1 - enter) * 4}px)`,
  }

  if (kind === 'ring') {
    return (
      <div style={style}>
        <div
          style={{
            width: 106,
            height: 106,
            borderRadius: 999,
            border: '22px solid rgba(255,255,255,0.86)',
            boxShadow:
              'inset 0 8px 20px rgba(17,17,17,0.08), 0 26px 54px rgba(17,17,17,0.12)',
            background: color,
          }}
        />
      </div>
    )
  }

  if (kind === 'wedge') {
    return (
      <div style={style}>
        <div
          style={{
            width: 138,
            height: 118,
            borderRadius: '30px 80px 34px 80px',
            background:
              'linear-gradient(135deg, rgba(255,255,255,0.96), rgba(216,231,255,0.74))',
            boxShadow: '0 32px 70px rgba(17,17,17,0.14)',
            transform: 'skewX(-8deg)',
          }}
        />
      </div>
    )
  }

  const dims =
    kind === 'tile'
      ? { width: 126, height: 126, radius: 30 }
      : { width: 190, height: 58, radius: 999 }

  return (
    <div style={style}>
      <div
        style={{
          width: dims.width,
          height: dims.height,
          borderRadius: dims.radius,
          background:
            kind === 'tile'
              ? 'linear-gradient(145deg, rgba(255,255,255,0.98), rgba(232,226,215,0.74))'
              : `linear-gradient(145deg, rgba(255,255,255,0.98), ${color})`,
          border: '1px solid rgba(255,255,255,0.72)',
          boxShadow:
            '0 28px 72px rgba(17,17,17,0.12), inset 0 1px 0 rgba(255,255,255,0.94)',
        }}
      />
    </div>
  )
}

function SourceCard({
  frame,
  index,
  label,
  title,
  meta,
  color,
  x,
  y,
  delay,
  rotate,
}: {
  frame: number
  index: number
  label: string
  title: string
  meta: string
  color: string
  x: number
  y: number
  delay: number
  rotate: number
}) {
  const enter = progress(frame, delay, delay + 36)
  const settle = progress(frame, delay + 24, delay + 78)
  const focusOut = progress(frame, 156, 210)
  const float = Math.sin((frame + index * 24) / 30) * 7
  return (
    <div
      style={{
        position: 'absolute',
        left: x,
        top: y,
        width: 380,
        height: 96,
        opacity: enter * interpolate(focusOut, [0, 1], [1, 0.66]),
        transform:
          `translate3d(${(1 - enter) * 80}px, ${(1 - enter) * 52 + float}px, 0) ` +
          `rotate(${rotate + (1 - settle) * 9}deg) scale(${0.82 + enter * 0.18})`,
        filter: `blur(${(1 - enter) * 7}px)`,
      }}
    >
      <div
        style={{
          width: '100%',
          height: '100%',
          borderRadius: 28,
          background:
            'linear-gradient(180deg, rgba(255,255,255,0.92), rgba(255,255,255,0.58))',
          border: '1px solid rgba(255,255,255,0.72)',
          boxShadow:
            '0 30px 80px rgba(17,17,17,0.13), inset 0 1px 0 rgba(255,255,255,0.92)',
          backdropFilter: 'blur(18px)',
          display: 'flex',
          alignItems: 'center',
          gap: 18,
          padding: '0 22px',
        }}
      >
        <div
          style={{
            width: 52,
            height: 52,
            borderRadius: 18,
            background: color,
            color: '#fff',
            display: 'grid',
            placeItems: 'center',
            fontSize: 18,
            fontWeight: 900,
            boxShadow: `0 16px 34px ${hexToRgba(color, 0.32)}`,
          }}
        >
          {label.slice(0, 1).toUpperCase()}
        </div>
        <div style={{ minWidth: 0 }}>
          <p
            style={{
              margin: 0,
              color: '#8A847B',
              fontSize: 12,
              letterSpacing: 2.1,
              textTransform: 'uppercase',
              fontWeight: 800,
            }}
          >
            {label}
          </p>
          <p style={{ margin: '5px 0 0', fontSize: 22, fontWeight: 850 }}>
            {title}
          </p>
        </div>
        <p
          style={{
            margin: '0 0 0 auto',
            color: MUTED,
            fontSize: 13,
            fontWeight: 700,
            maxWidth: 112,
            lineHeight: 1.25,
            textAlign: 'right',
          }}
        >
          {meta}
        </p>
      </div>
    </div>
  )
}

function KnowledgeGraph({ frame }: { frame: number }) {
  const enter = progress(frame, 52, 112)
  const focusOut = progress(frame, 156, 214)
  const panelRotate = interpolate(enter, [0, 1], [9, -2])
  return (
    <div
      style={{
        position: 'absolute',
        left: 615,
        top: 218,
        width: 710,
        height: 510,
        opacity: enter * interpolate(focusOut, [0, 1], [1, 0.58]),
        transform:
          `perspective(1200px) rotateX(10deg) rotateY(${panelRotate}deg) ` +
          `translate3d(${(1 - enter) * 54}px, ${(1 - enter) * 36}px, 0) scale(${0.82 + enter * 0.18})`,
      }}
    >
      <GlassPlate>
        <div
          style={{
            position: 'absolute',
            left: 34,
            top: 30,
            zIndex: 2,
          }}
        >
          <p
            style={{
              margin: 0,
              color: '#8A847B',
              fontSize: 12,
              letterSpacing: 2,
              textTransform: 'uppercase',
              fontWeight: 850,
            }}
          >
            Knowledge graph
          </p>
          <p style={{ margin: '8px 0 0', fontSize: 28, fontWeight: 850 }}>
            Company memory is forming
          </p>
        </div>
        <svg
          viewBox="0 0 100 100"
          style={{
            position: 'absolute',
            left: 78,
            top: 74,
            width: 560,
            height: 390,
            overflow: 'visible',
          }}
        >
          {graphEdges.map(([from, to], index) => {
            const a = graphNodes.find((node) => node.id === from)
            const b = graphNodes.find((node) => node.id === to)
            if (!a || !b) return null
            const edge = progress(frame, 68 + index * 5, 104 + index * 5)
            return (
              <line
                key={`${from}-${to}`}
                x1={a.x}
                y1={a.y}
                x2={interpolate(edge, [0, 1], [a.x, b.x])}
                y2={interpolate(edge, [0, 1], [a.y, b.y])}
                stroke={TEXT}
                strokeOpacity={0.18}
                strokeWidth={0.55}
                strokeLinecap="round"
              />
            )
          })}
          {graphNodes.map((node, index) => {
            const nodeIn = progress(frame, 58 + index * 8, 88 + index * 8)
            const pulse = Math.sin((frame + index * 18) / 16) * 0.5 + 0.5
            return (
              <g key={node.id} opacity={nodeIn}>
                <circle
                  cx={node.x}
                  cy={node.y}
                  r={node.r + pulse * 0.55}
                  fill={node.color}
                  opacity={0.18}
                />
                <circle
                  cx={node.x}
                  cy={node.y}
                  r={node.r}
                  fill={node.color}
                  filter="drop-shadow(0 7px 9px rgba(17,17,17,0.16))"
                />
                <text
                  x={node.x}
                  y={node.y + node.r + 6.5}
                  textAnchor="middle"
                  fill={TEXT}
                  fillOpacity={0.58}
                  fontSize={3.5}
                  fontWeight={800}
                >
                  {node.label}
                </text>
              </g>
            )
          })}
        </svg>
      </GlassPlate>
    </div>
  )
}

function MetricCluster({ frame }: { frame: number }) {
  const enter = progress(frame, 92, 148)
  const focusOut = progress(frame, 162, 214)
  const tokenWidth = interpolate(enter, [0, 1], [14, 78])
  const chart = progress(frame, 106, 160)
  return (
    <div
      style={{
        position: 'absolute',
        left: 697,
        top: 700,
        display: 'flex',
        gap: 18,
        opacity: enter * interpolate(focusOut, [0, 1], [1, 0.48]),
        transform: `translateY(${(1 - enter) * 34}px) scale(${0.94 + enter * 0.06})`,
      }}
    >
      <MetricCard title="Token cost" value="0.18 kr">
        <div
          style={{
            width: 214,
            height: 12,
            borderRadius: 999,
            background: '#E5DED2',
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              height: '100%',
              width: `${tokenWidth}%`,
              borderRadius: 999,
              background: CORAL,
              boxShadow: `0 0 18px ${hexToRgba(CORAL, 0.38)}`,
            }}
          />
        </div>
      </MetricCard>
      <MetricCard title="CSAT" value="94%">
        <div style={{ display: 'flex', gap: 7, alignItems: 'flex-end', height: 74 }}>
          {[0.45, 0.68, 0.54, 0.92].map((height, index) => (
            <div
              key={index}
              style={{
                width: 22,
                height: Math.max(12, chart * height * 76),
                borderRadius: 8,
                background: index === 3 ? CORAL : '#D9D2C6',
              }}
            />
          ))}
        </div>
      </MetricCard>
    </div>
  )
}

function MetricCard({
  title,
  value,
  children,
}: {
  title: string
  value: string
  children: ReactNode
}) {
  return (
    <div
      style={{
        width: 272,
        height: 142,
        borderRadius: 28,
        background:
          'linear-gradient(180deg, rgba(255,255,255,0.94), rgba(255,255,255,0.62))',
        border: '1px solid rgba(255,255,255,0.70)',
        boxShadow: '0 26px 70px rgba(17,17,17,0.12)',
        padding: '22px 24px',
      }}
    >
      <p
        style={{
          margin: 0,
          color: '#8A847B',
          fontSize: 12,
          letterSpacing: 1.8,
          textTransform: 'uppercase',
          fontWeight: 850,
        }}
      >
        {title}
      </p>
      <p style={{ margin: '8px 0 18px', fontSize: 30, fontWeight: 900 }}>
        {value}
      </p>
      {children}
    </div>
  )
}

function FinalReplyCard({ frame, focus }: { frame: number; focus: number }) {
  const enter = progress(frame, 112, 158)
  const typing = progress(frame, 132, 180)
  const citationIn = progress(frame, 158, 198)
  const scale = interpolate(focus, [0, 1], [0.9, 1])
  const y = interpolate(focus, [0, 1], [44, 0])

  return (
    <div
      style={{
        position: 'absolute',
        left: '50%',
        top: '50%',
        width: 900,
        height: 438,
        marginLeft: -450,
        marginTop: -218,
        opacity: enter,
        transform:
          `translate3d(0, ${(1 - enter) * 54 + y}px, 0) scale(${0.86 + enter * 0.1 + scale * 0.04})`,
        filter: `blur(${(1 - enter) * 7}px)`,
      }}
    >
      <div
        style={{
          position: 'absolute',
          inset: 0,
          borderRadius: 36,
          background:
            'linear-gradient(180deg, rgba(255,255,255,0.98), rgba(255,255,255,0.82))',
          border: '1px solid rgba(255,255,255,0.82)',
          boxShadow:
            '0 44px 120px rgba(17,17,17,0.22), inset 0 1px 0 rgba(255,255,255,0.96)',
        }}
      />
      <div
        style={{
          position: 'absolute',
          inset: 18,
          borderRadius: 28,
          border: `1px solid ${LINE}`,
          overflow: 'hidden',
          background: PAPER,
        }}
      >
        <div
          style={{
            height: 86,
            borderBottom: `1px solid ${LINE}`,
            display: 'flex',
            alignItems: 'center',
            padding: '0 34px',
            gap: 16,
          }}
        >
          <div
            style={{
              width: 42,
              height: 42,
              borderRadius: 16,
              background: TEXT,
              color: '#fff',
              display: 'grid',
              placeItems: 'center',
              fontWeight: 900,
              boxShadow: '0 14px 30px rgba(17,17,17,0.22)',
            }}
          >
            V
          </div>
          <div>
            <p style={{ margin: 0, fontSize: 22, fontWeight: 900 }}>
              Verevon agent
            </p>
            <p style={{ margin: '5px 0 0', color: MUTED, fontSize: 14 }}>
              Answer grounded in your connected sources
            </p>
          </div>
          <div
            style={{
              marginLeft: 'auto',
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              color: '#7E776F',
              fontSize: 13,
              fontWeight: 800,
            }}
          >
            <span
              style={{
                width: 10,
                height: 10,
                borderRadius: 999,
                background: CORAL,
                boxShadow: `0 0 20px ${hexToRgba(CORAL, 0.72)}`,
              }}
            />
            Live reply
          </div>
        </div>

        <div style={{ padding: '32px 36px 0', display: 'grid', gridTemplateColumns: '1.1fr 0.9fr', gap: 28 }}>
          <div>
            <p
              style={{
                margin: 0,
                fontFamily: '"Cormorant Garamond", Georgia, serif',
                fontSize: 48,
                lineHeight: 0.98,
                letterSpacing: 0,
              }}
            >
              “Here is the answer we can safely send.”
            </p>
            <div style={{ marginTop: 25, display: 'flex', flexDirection: 'column', gap: 12 }}>
              <TypingLine width={364} progress={typing} delay={0} />
              <TypingLine width={318} progress={typing} delay={0.16} />
              <TypingLine width={398} progress={typing} delay={0.32} coral />
            </div>
          </div>

          <div
            style={{
              height: 222,
              borderRadius: 24,
              background: 'linear-gradient(180deg, #151515, #0F0F10)',
              color: '#fff',
              padding: 22,
              boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.08)',
            }}
          >
            <p
              style={{
                margin: 0,
                color: 'rgba(255,255,255,0.48)',
                fontSize: 12,
                letterSpacing: 1.8,
                textTransform: 'uppercase',
                fontWeight: 850,
              }}
            >
              Sources used
            </p>
            <div style={{ marginTop: 18, display: 'flex', flexDirection: 'column', gap: 10 }}>
              <SourceChip label="triodelab.no" color={CORAL} progress={citationIn} index={0} />
              <SourceChip label="Microsoft 365" color={BLUE} progress={citationIn} index={1} />
              <SourceChip label="Company registry" color={GOLD} progress={citationIn} index={2} />
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

function TypingLine({
  width,
  progress: amount,
  delay,
  coral,
}: {
  width: number
  progress: number
  delay: number
  coral?: boolean
}) {
  const local = interpolate(amount, [delay, delay + 0.42], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  })
  return (
    <div
      style={{
        width,
        height: 12,
        borderRadius: 999,
        background: '#E9E3D8',
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          height: '100%',
          width: `${local * 100}%`,
          background: coral ? CORAL : TEXT,
          borderRadius: 999,
          opacity: coral ? 0.86 : 0.62,
        }}
      />
    </div>
  )
}

function SourceChip({
  label,
  color,
  progress: amount,
  index,
}: {
  label: string
  color: string
  progress: number
  index: number
}) {
  const local = interpolate(amount, [index * 0.16, index * 0.16 + 0.38], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  })
  return (
    <div
      style={{
        height: 42,
        borderRadius: 999,
        background: 'rgba(255,255,255,0.08)',
        border: '1px solid rgba(255,255,255,0.10)',
        display: 'flex',
        alignItems: 'center',
        padding: '0 14px',
        gap: 10,
        opacity: local,
        transform: `translateX(${(1 - local) * 22}px)`,
      }}
    >
      <span
        style={{
          width: 11,
          height: 11,
          borderRadius: 999,
          background: color,
          boxShadow: `0 0 14px ${hexToRgba(color, 0.65)}`,
        }}
      />
      <span style={{ fontSize: 14, fontWeight: 800 }}>{label}</span>
    </div>
  )
}

function GlassPlate({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        borderRadius: 40,
        background:
          'linear-gradient(180deg, rgba(255,255,255,0.86), rgba(255,255,255,0.52))',
        border: '1px solid rgba(255,255,255,0.72)',
        boxShadow:
          '0 42px 110px rgba(17,17,17,0.14), inset 0 1px 0 rgba(255,255,255,0.94)',
        backdropFilter: 'blur(22px)',
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          position: 'absolute',
          inset: 0,
          background:
            'radial-gradient(circle at 50% 45%, rgba(255,255,255,0.64), rgba(255,255,255,0) 60%)',
        }}
      />
      {children}
    </div>
  )
}

function Wordmark({ frame }: { frame: number }) {
  const opacity = interpolate(frame, [0, 28, 184, 224], [0, 0.72, 0.72, 0.35], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  return (
    <div
      style={{
        position: 'absolute',
        left: 112,
        top: 86,
        opacity,
        display: 'flex',
        alignItems: 'center',
        gap: 14,
      }}
    >
      <div
        style={{
          width: 36,
          height: 36,
          borderRadius: 13,
          background: TEXT,
          color: '#fff',
          display: 'grid',
          placeItems: 'center',
          fontSize: 17,
          fontWeight: 900,
          boxShadow: '0 18px 40px rgba(17,17,17,0.18)',
        }}
      >
        V
      </div>
      <span
        style={{
          fontFamily: '"Cormorant Garamond", Georgia, serif',
          fontSize: 40,
          lineHeight: 1,
        }}
      >
        Verevon
      </span>
    </div>
  )
}

function Noise() {
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        opacity: 0.06,
        mixBlendMode: 'multiply',
        backgroundImage:
          'url("data:image/svg+xml,%3Csvg viewBox=%270 0 180 180%27 xmlns=%27http://www.w3.org/2000/svg%27%3E%3Cfilter id=%27n%27%3E%3CfeTurbulence type=%27fractalNoise%27 baseFrequency=%270.86%27 numOctaves=%274%27 stitchTiles=%27stitch%27/%3E%3C/filter%3E%3Crect width=%27180%27 height=%27180%27 filter=%27url(%23n)%27 opacity=%270.55%27/%3E%3C/svg%3E")',
        backgroundSize: '180px 180px',
        pointerEvents: 'none',
      }}
    />
  )
}

function LensVignette() {
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        pointerEvents: 'none',
        boxShadow: 'inset 0 0 150px rgba(17,17,17,0.08)',
        background:
          'radial-gradient(ellipse at 50% 48%, rgba(255,255,255,0) 58%, rgba(17,17,17,0.08) 100%)',
      }}
    />
  )
}

function progress(frame: number, from: number, to: number): number {
  return interpolate(frame, [from, to], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.bezier(0.16, 1, 0.3, 1),
  })
}

function hexToRgba(hex: string, alpha: number): string {
  const normalized = hex.replace('#', '')
  const r = parseInt(normalized.slice(0, 2), 16)
  const g = parseInt(normalized.slice(2, 4), 16)
  const b = parseInt(normalized.slice(4, 6), 16)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}
