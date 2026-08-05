import type { CSSProperties } from 'react'
import {
  AbsoluteFill,
  Easing,
  OffthreadVideo,
  interpolate,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion'

const scenes = [
  {
    start: 0.0,
    end: 2.4,
    kicker: '01 / Signal',
    title: 'Fra kundesignal til godkjent handling.',
    side: -1,
  },
  {
    start: 2.4,
    end: 5.3,
    kicker: '02 / Inngang',
    title: 'Signalene samles',
    bullets: ['E-post', 'Slack', 'Dokumenter', 'Brreg'],
    side: 1,
  },
  {
    start: 5.3,
    end: 8.8,
    kicker: '03 / Utkast',
    title: 'Verevon bygger et kildebelagt svarutkast.',
    side: -1,
  },
  {
    start: 8.8,
    end: 11.8,
    kicker: '04 / Kontroll',
    title: 'Mennesket tar siste steg.',
    bullets: ['Sporbart', 'Trygt', 'Kontrollert'],
    side: 1,
  },
] as const

const points = [
  { x: 900, y: 782 },
  { x: 990, y: 702 },
  { x: 1064, y: 590 },
  { x: 1210, y: 518 },
]

const path =
  'M 870 816 C 900 784 938 752 978 724 C 1040 672 1002 626 1064 590 C 1130 550 1182 540 1250 508'

const WHITE = 'rgba(255,255,255,0.96)'
const CORAL = '#ee7a50'

type Scene = (typeof scenes)[number]
type Point = { x: number; y: number }

const depthPlanes = [
  {
    start: 0.05,
    end: 2.55,
    label: 'Signal',
    accent: '#f4bf73',
    blob: 'rgba(244,191,115,0.22)',
    x: 1110,
    y: 166,
    side: 1,
  },
  {
    start: 2.35,
    end: 5.35,
    label: 'Inngang',
    accent: '#7ec8ff',
    blob: 'rgba(126,200,255,0.24)',
    x: 255,
    y: 118,
    side: -1,
  },
  {
    start: 5.25,
    end: 8.95,
    label: 'Utkast',
    accent: '#ee7a50',
    blob: 'rgba(238,122,80,0.22)',
    x: 1130,
    y: 250,
    side: 1,
  },
  {
    start: 8.65,
    end: 11.95,
    label: 'Kontroll',
    accent: '#d9e6c2',
    blob: 'rgba(217,230,194,0.2)',
    x: 290,
    y: 240,
    side: -1,
  },
] as const

const curveSegments = [
  [
    { x: 870, y: 816 },
    { x: 900, y: 784 },
    { x: 938, y: 752 },
    { x: 978, y: 724 },
  ],
  [
    { x: 978, y: 724 },
    { x: 1040, y: 672 },
    { x: 1002, y: 626 },
    { x: 1064, y: 590 },
  ],
  [
    { x: 1064, y: 590 },
    { x: 1130, y: 550 },
    { x: 1180, y: 540 },
    { x: 1250, y: 508 },
  ],
] as const

const signalDepthSegments = [
  {
    d: 'M 870 816 C 900 784 938 752 978 724',
    core: 2.4,
    glow: 10,
    shadow: 12,
    offsetX: 5,
    offsetY: 8,
  },
  {
    d: 'M 978 724 C 1040 672 1002 626 1064 590',
    core: 2.2,
    glow: 12,
    shadow: 15,
    offsetX: 10,
    offsetY: 15,
  },
  {
    d: 'M 1064 590 C 1130 550 1180 540 1250 508',
    core: 1.5,
    glow: 8,
    shadow: 10,
    offsetX: 5,
    offsetY: 8,
  },
] as const

export function VerevonSignalDroneShort() {
  return (
    <AbsoluteFill
      style={{
        background: '#0b0b0a',
        color: WHITE,
        fontFamily:
          'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
        overflow: 'hidden',
      }}
    >
      <SignalTunnel />
      <DepthGalleryAtmosphere />
      <DepthGalleryPlanes />
      <SignalPath />
      <ForegroundTerrainOcclusion />

      {scenes.map((scene, index) => (
        <CheckpointModal key={scene.kicker} scene={scene} index={index} />
      ))}

      <FinalLockup />
    </AbsoluteFill>
  )
}

function SignalTunnel() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const drift = interpolate(frame, [0, 14.04 * fps], [1.03, 1.09], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })

  return (
    <>
      <OffthreadVideo
        src={staticFile('videos/verevon-hero-premiere-light.mp4')}
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          objectFit: 'cover',
          transform: `scale(${drift})`,
          filter: 'saturate(1.08) contrast(1.03)',
        }}
        volume={0}
      />
      <div
        style={{
          position: 'absolute',
          inset: 0,
          background:
            'radial-gradient(circle at 54% 42%, rgba(255,255,255,0.08), transparent 34%), radial-gradient(circle at 48% 74%, rgba(238,122,80,0.16), transparent 28%)',
          mixBlendMode: 'screen',
          pointerEvents: 'none',
        }}
      />
    </>
  )
}

function DepthGalleryAtmosphere() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()

  return (
    <AbsoluteFill style={{ pointerEvents: 'none' }}>
      {depthPlanes.map((plane, index) => {
        const opacity = interpolate(
          frame,
          [(plane.start - 0.35) * fps, plane.start * fps, plane.end * fps, (plane.end + 0.55) * fps],
          [0, 0.58, 0.5, 0],
          {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          }
        )
        const drift = interpolate(
          frame,
          [plane.start * fps, plane.end * fps],
          [index % 2 === 0 ? -3 : 3, index % 2 === 0 ? 5 : -5],
          {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          }
        )

        return (
          <div
            key={plane.label}
            style={{
              position: 'absolute',
              inset: '-12%',
              background: `radial-gradient(circle at ${plane.side > 0 ? 64 : 36}% ${index % 2 === 0 ? 30 : 68}%, ${plane.blob}, transparent 32%)`,
              filter: 'blur(22px)',
              mixBlendMode: 'screen',
              opacity,
              transform: `translate3d(${drift * 12}px, ${drift * -7}px, 0)`,
            }}
          />
        )
      })}
    </AbsoluteFill>
  )
}

function DepthGalleryPlanes() {
  return (
    <AbsoluteFill
      style={{
        perspective: 1050,
        pointerEvents: 'none',
        transformStyle: 'preserve-3d',
      }}
    >
      {depthPlanes.map((plane, index) => (
        <DepthPlane key={plane.label} index={index} plane={plane} />
      ))}
    </AbsoluteFill>
  )
}

function DepthPlane({
  index,
  plane,
}: {
  index: number
  plane: (typeof depthPlanes)[number]
}) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const start = plane.start * fps
  const end = plane.end * fps
  const active = interpolate(frame, [start - 14, start + 14, end - 16, end + 18], [0, 1, 1, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const travel = interpolate(frame, [start - 14, end + 18], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.bezier(0.16, 0.76, 0.18, 1),
  })
  const x = interpolate(travel, [0, 0.5, 1], [plane.side * 210, 0, plane.side * -160])
  const y = interpolate(travel, [0, 1], [34, -28])
  const z = interpolate(travel, [0, 0.42, 1], [-760, -210, 120])
  const rotateY = interpolate(travel, [0, 0.52, 1], [plane.side * -16, plane.side * 2, plane.side * 11])
  const rotateX = interpolate(travel, [0, 1], [7, -5])
  const scale = interpolate(travel, [0, 0.45, 1], [0.5, 0.7, 0.9])
  const blur = interpolate(travel, [0, 0.42, 1], [6, 1.2, 4])
  const paneOpacity = active * interpolate(travel, [0, 0.42, 1], [0.06, 0.22, 0.12])
  const ghostOpacity = active * interpolate(travel, [0, 0.5, 1], [0.04, 0.11, 0.04])

  return (
    <>
      <div
        style={{
          ...depthGhostStyle,
          borderColor: plane.accent,
          left: plane.x + plane.side * -72,
          opacity: ghostOpacity,
          top: plane.y + 48,
          transform: `translate3d(${x * 0.78}px, ${y * 0.64}px, ${z - 210}px) rotateY(${rotateY * 0.8}deg) rotateX(${rotateX}deg) scale(${scale * 0.94})`,
        }}
      />
      <div
        style={{
          ...depthPlaneStyle,
          borderColor: `${plane.accent}66`,
          boxShadow: `0 44px 130px rgba(0,0,0,0.16), 0 0 64px ${plane.blob}`,
          filter: `blur(${blur}px)`,
          left: plane.x,
          opacity: paneOpacity,
          top: plane.y,
          transform: `translate3d(${x}px, ${y}px, ${z}px) rotateY(${rotateY}deg) rotateX(${rotateX}deg) scale(${scale})`,
        }}
      >
        <div
          style={{
            background: `radial-gradient(circle at ${plane.side > 0 ? 28 : 72}% 32%, ${plane.blob}, transparent 34%), linear-gradient(135deg, rgba(255,255,255,0.18), rgba(255,255,255,0.025))`,
            borderRadius: 26,
            height: '100%',
            overflow: 'hidden',
            position: 'relative',
          }}
        >
          <div
            style={{
              background:
                'linear-gradient(90deg, transparent, rgba(255,255,255,0.42), transparent)',
              height: 1,
              left: '10%',
              opacity: 0.52,
              position: 'absolute',
              top: `${30 + index * 9}%`,
              transform: `rotate(${plane.side * -6}deg)`,
              width: '76%',
            }}
          />
          <div
            style={{
              background: plane.accent,
              borderRadius: 999,
              boxShadow: `0 0 22px ${plane.accent}`,
              height: 8,
              left: plane.side > 0 ? '18%' : '78%',
              opacity: 0.72,
              position: 'absolute',
              top: '24%',
              width: 8,
            }}
          />
          <div
            style={{
              border: '1px solid rgba(255,255,255,0.2)',
              borderRadius: 999,
              bottom: '20%',
              height: 96,
              opacity: 0.24,
              position: 'absolute',
              right: plane.side > 0 ? '18%' : '62%',
              width: 96,
            }}
          />
        </div>
      </div>
    </>
  )
}

function SignalPath() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const draw = interpolate(frame, [0.45 * fps, 11.8 * fps], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.bezier(0.22, 0.74, 0.18, 1),
  })
  const pathOpacity = interpolate(
    frame,
    [0, 0.45 * fps, 11.8 * fps, 13 * fps],
    [0, 1, 1, 0],
    {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    }
  )
  const cameraScale = interpolate(frame, [0, 14.04 * fps], [1.01, 1.06], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const cameraX = interpolate(frame, [0, 14.04 * fps], [-10, -34], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const cameraY = interpolate(frame, [0, 14.04 * fps], [12, -8], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const tilt = interpolate(frame, [0, 14.04 * fps], [12, 9], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const cometLength = interpolate(draw, [0, 1], [0.09, 0.145], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const head = getSignalPoint(draw)

  return (
    <AbsoluteFill
      style={{
        perspective: 1200,
        pointerEvents: 'none',
        transformStyle: 'preserve-3d',
      }}
    >
      <svg
        viewBox="0 0 1920 1080"
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          opacity: pathOpacity,
          overflow: 'visible',
          transform: `translate3d(${cameraX}px, ${cameraY}px, 0) scale(${cameraScale}) rotateX(${tilt}deg) rotateZ(-0.75deg)`,
          transformOrigin: '50% 82%',
        }}
      >
        {signalDepthSegments.map((segment, index) => {
          const start = index / signalDepthSegments.length
          const end = (index + 1) / signalDepthSegments.length
          const segmentDraw = interpolate(draw, [start, end], [0, 1], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          })
          const segmentOpacity = interpolate(segmentDraw, [0, 0.04], [0, 1], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          })
          const distanceOpacity = [0.54, 0.66, 0.46][index]

          return (
            <g key={segment.d} opacity={segmentOpacity * distanceOpacity}>
              <path
                d={segment.d}
                fill="none"
                pathLength={1}
                stroke="rgba(255,255,255,0.12)"
                strokeLinecap="round"
                strokeWidth={segment.core * 1.8}
              />
              <path
                d={segment.d}
                fill="none"
                pathLength={1}
                stroke="rgba(0,0,0,0.2)"
                strokeDasharray={1}
                strokeDashoffset={1 - segmentDraw}
                strokeLinecap="round"
                strokeWidth={segment.shadow}
                style={{
                  filter: 'blur(16px)',
                  transform: `translate(${segment.offsetX}px, ${segment.offsetY}px)`,
                }}
              />
              <path
                d={segment.d}
                fill="none"
                pathLength={1}
                stroke="rgba(255,255,255,0.36)"
                strokeDasharray={1}
                strokeDashoffset={1 - segmentDraw}
                strokeLinecap="round"
                strokeWidth={segment.glow}
                style={{ filter: 'blur(13px)' }}
              />
              <path
                d={segment.d}
                fill="none"
                pathLength={1}
                stroke="rgba(255,255,255,0.9)"
                strokeDasharray={1}
                strokeDashoffset={1 - segmentDraw}
                strokeLinecap="round"
                strokeWidth={segment.core}
              />
            </g>
          )
        })}
        <path
          d={path}
          fill="none"
          pathLength={1}
          stroke={CORAL}
          strokeDasharray={`${cometLength * 0.33} 1`}
          strokeDashoffset={1 - draw}
          strokeLinecap="round"
          strokeWidth={2.2}
        />

        <SignalHead draw={draw} point={head} />

        {points.map((point, index) => (
          <CheckpointPoint key={`${point.x}-${point.y}`} index={index} point={point} />
        ))}
      </svg>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          background:
            'radial-gradient(circle at 51% 76%, rgba(255,255,255,0.12), transparent 18%), radial-gradient(circle at 53% 36%, rgba(255,255,255,0.08), transparent 22%)',
          mixBlendMode: 'screen',
          opacity: pathOpacity * 0.62,
        }}
      />
    </AbsoluteFill>
  )
}

function ForegroundTerrainOcclusion() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const opacity = interpolate(
    frame,
    [0, 0.45 * fps, 11.8 * fps, 13 * fps],
    [0, 0.72, 0.62, 0],
    {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    }
  )

  return (
    <AbsoluteFill
      style={{
        background:
          'radial-gradient(ellipse at 44% 91%, rgba(8,8,6,0.34), transparent 22%), radial-gradient(ellipse at 18% 100%, rgba(8,8,6,0.24), transparent 28%), linear-gradient(to top, rgba(8,8,6,0.16), transparent 18%)',
        mixBlendMode: 'multiply',
        opacity,
        pointerEvents: 'none',
      }}
    />
  )
}

function SignalHead({ draw, point }: { draw: number; point: Point }) {
  const depthScale = interpolate(draw, [0, 0.48, 1], [0.98, 0.86, 0.45], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const opacity = interpolate(draw, [0, 0.025, 0.98, 1], [0, 1, 1, 0.52], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })

  return (
    <g opacity={opacity}>
      <circle
        cx={point.x}
        cy={point.y}
        fill="rgba(255,255,255,0.2)"
        r={28 * depthScale}
        style={{ filter: 'blur(12px)' }}
      />
      <circle
        cx={point.x}
        cy={point.y}
        fill="rgba(238,122,80,0.42)"
        r={18 * depthScale}
        style={{ filter: 'blur(8px)' }}
      />
      <circle cx={point.x} cy={point.y} fill={WHITE} r={5.4 * depthScale} />
      <circle cx={point.x} cy={point.y} fill={CORAL} r={2.6 * depthScale} />
    </g>
  )
}

function CheckpointPoint({
  index,
  point,
}: {
  index: number
  point: Point
}) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const hit = [0.45, 2.7, 5.6, 9.0][index] * fps
  const pulse = interpolate(frame, [hit - 8, hit, hit + 18], [0, 1, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const base = interpolate(frame, [hit - 10, hit + 8], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })
  const depthScale = [0.72, 0.78, 0.66, 0.5][index]

  return (
    <g opacity={base}>
      <circle
        cx={point.x}
        cy={point.y}
        fill="rgba(255,255,255,0.14)"
        r={(34 + pulse * 34) * depthScale}
        style={{ filter: 'blur(11px)' }}
      />
      <circle
        cx={point.x}
        cy={point.y}
        fill="none"
        r={(18 + pulse * 24) * depthScale}
        stroke="rgba(255,255,255,0.46)"
        strokeWidth={2}
      />
      <circle cx={point.x} cy={point.y} fill={WHITE} r={(4.5 + pulse * 2.2) * depthScale} />
      <circle
        cx={point.x}
        cy={point.y}
        fill="rgba(238,122,80,0.88)"
        r={(2.2 + pulse * 2.4) * depthScale}
      />
    </g>
  )
}

function getSignalPoint(progress: number): Point {
  const safeProgress = Math.max(0, Math.min(1, progress))
  const segmentIndex = Math.min(
    curveSegments.length - 1,
    Math.floor(safeProgress * curveSegments.length)
  )
  const segmentProgress = safeProgress * curveSegments.length - segmentIndex
  const [start, controlA, controlB, end] = curveSegments[segmentIndex]

  return cubicBezierPoint(start, controlA, controlB, end, segmentProgress)
}

function cubicBezierPoint(
  start: Point,
  controlA: Point,
  controlB: Point,
  end: Point,
  progress: number
): Point {
  const inverse = 1 - progress
  const x =
    inverse ** 3 * start.x +
    3 * inverse ** 2 * progress * controlA.x +
    3 * inverse * progress ** 2 * controlB.x +
    progress ** 3 * end.x
  const y =
    inverse ** 3 * start.y +
    3 * inverse ** 2 * progress * controlA.y +
    3 * inverse * progress ** 2 * controlB.y +
    progress ** 3 * end.y

  return { x, y }
}

function CheckpointModal({ scene, index }: { scene: Scene; index: number }) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const start = scene.start * fps
  const end = scene.end * fps
  const side = scene.side
  const bullets = 'bullets' in scene ? scene.bullets : undefined
  const opacity = interpolate(
    frame,
    [start, start + 13, end - 14, end],
    [0, 1, 1, 0],
    {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    }
  )
  const y = [604, 548, 432, 300][index]
  const x = side < 0 ? 180 : 1110
  const slide = interpolate(frame, [start, start + 18], [side * -42, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  })
  const scale = interpolate(frame, [start, start + 18], [0.96, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  })

  return (
    <div
      style={{
        position: 'absolute',
        left: x,
        top: y,
        width: bullets ? 560 : 515,
        opacity,
        transform: `translate3d(${slide}px, 0, 0) scale(${scale})`,
        transformOrigin: side < 0 ? '0% 50%' : '100% 50%',
        ...glassCard,
      }}
    >
      <div
        style={{
          color: 'rgba(255,255,255,0.56)',
          fontSize: 16,
          fontWeight: 700,
          letterSpacing: 1.8,
          marginBottom: 18,
          textTransform: 'uppercase',
        }}
      >
        {scene.kicker}
      </div>
      <div
        style={{
          color: WHITE,
          fontSize: scene.title.length > 38 ? 34 : 40,
          fontWeight: 720,
          letterSpacing: -1.25,
          lineHeight: 1.04,
        }}
      >
        {scene.title}
      </div>
      {bullets ? (
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            gap: 10,
            marginTop: 24,
          }}
        >
          {bullets.map((bullet, bulletIndex) => (
            <div
              key={bullet}
              style={{
                border: '1px solid rgba(255,255,255,0.18)',
                borderRadius: 999,
                color: 'rgba(255,255,255,0.78)',
                fontSize: 18,
                fontWeight: 650,
                letterSpacing: -0.2,
                padding: '10px 14px',
                transform: `translateY(${interpolate(
                  frame,
                  [start + 10 + bulletIndex * 3, start + 22 + bulletIndex * 3],
                  [10, 0],
                  {
                    extrapolateLeft: 'clamp',
                    extrapolateRight: 'clamp',
                  }
                )}px)`,
                opacity: interpolate(
                  frame,
                  [start + 9 + bulletIndex * 3, start + 20 + bulletIndex * 3],
                  [0, 1],
                  {
                    extrapolateLeft: 'clamp',
                    extrapolateRight: 'clamp',
                  }
                ),
              }}
            >
              {bullet}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  )
}

function FinalLockup() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()

  const start = 11.8 * fps

  const opacity = interpolate(frame, [start, start + 14, start + 60], [0, 1, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })

  const scale = interpolate(frame, [start, start + 24], [0.94, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  })

  return (
    <AbsoluteFill
      style={{
        justifyContent: 'center',
        alignItems: 'center',
        color: 'white',
        opacity,
        transform: `scale(${scale})`,
        textAlign: 'center',
        pointerEvents: 'none',
      }}
    >
      <div
        style={{
          fontSize: 82,
          fontWeight: 700,
          letterSpacing: -2.4,
          textShadow: '0 0 34px rgba(255,255,255,0.28)',
        }}
      >
        Verevon
      </div>

      <div
        style={{
          marginTop: 18,
          fontSize: 34,
          opacity: 0.82,
          letterSpacing: -0.4,
        }}
      >
        AI som handler med kontroll.
      </div>
    </AbsoluteFill>
  )
}

const depthPlaneStyle: CSSProperties = {
  background:
    'linear-gradient(145deg, rgba(255,255,255,0.12), rgba(255,255,255,0.045))',
  border: '1px solid rgba(255,255,255,0.18)',
  borderRadius: 30,
  height: 390,
  padding: '32px 34px',
  position: 'absolute',
  transformOrigin: '50% 50%',
  transformStyle: 'preserve-3d',
  width: 430,
  backdropFilter: 'blur(18px)',
}

const depthGhostStyle: CSSProperties = {
  background:
    'linear-gradient(145deg, rgba(255,255,255,0.06), rgba(255,255,255,0.02))',
  border: '1px solid rgba(255,255,255,0.12)',
  borderRadius: 30,
  filter: 'blur(1.2px)',
  height: 390,
  position: 'absolute',
  transformOrigin: '50% 50%',
  transformStyle: 'preserve-3d',
  width: 430,
  backdropFilter: 'blur(8px)',
}

const glassCard: CSSProperties = {
  background:
    'linear-gradient(135deg, rgba(8,8,8,0.58), rgba(8,8,8,0.32))',
  border: '1px solid rgba(255,255,255,0.16)',
  borderRadius: 34,
  boxShadow:
    '0 30px 80px rgba(0,0,0,0.34), inset 0 1px 0 rgba(255,255,255,0.14)',
  padding: '34px 38px 36px',
  backdropFilter: 'blur(18px)',
}
