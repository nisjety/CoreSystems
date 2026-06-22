type VelionCircuitDiagramProps = {
  animateLines?: boolean;
  animateSignals?: boolean;
  className?: string;
  text?: string;
};

const paths = [
  { d: "M 10 20 h 78 q 6 0 6 6 v 20", id: "one", signal: "copper", time: "7s" },
  { d: "M 184 12 h -72 q -6 0 -6 6 v 28", id: "two", signal: "blue", time: "8s" },
  { d: "M 132 20 v 20 q 0 6 -6 6 h -12", id: "three", signal: "ink", time: "6.5s" },
  { d: "M 174 82 v -22 q 0 -6 -6 -6 h -48", id: "four", signal: "copper", time: "8.6s" },
  { d: "M 136 66 h 16 q 6 0 6 6 v 9 q 0 6 -6 6 h -38 q -6 0 -6 -6 v -20", id: "five", signal: "blue", time: "7.8s" },
  { d: "M 98 95 v -34", id: "six", signal: "ink", time: "6.8s" },
  { d: "M 88 88 v -14 q 0 -6 -6 -6 h -10 q -6 0 -6 -6 v -4 q 0 -6 6 -6 h 16", id: "seven", signal: "copper", time: "8.2s" },
  { d: "M 30 30 h 26 q 6 0 6 6 v 5 q 0 6 6 6 h 20", id: "eight", signal: "blue", time: "7.4s" },
];

export function VelionCircuitDiagram({
  animateLines = true,
  animateSignals = true,
  className = "",
  text = "VELION",
}: VelionCircuitDiagramProps) {
  return (
    <svg
      aria-hidden="true"
      className={`velion-circuit-diagram ${className}`.trim()}
      preserveAspectRatio="xMidYMid meet"
      viewBox="0 0 200 100"
    >
      <defs>
        <radialGradient id="velion-circuit-copper-gradient">
          <stop offset="0%" stopColor="#d59a7a" />
          <stop offset="58%" stopColor="#8a4a32" />
          <stop offset="100%" stopColor="transparent" />
        </radialGradient>
        <radialGradient id="velion-circuit-blue-gradient">
          <stop offset="0%" stopColor="#d9edf2" />
          <stop offset="52%" stopColor="#527184" />
          <stop offset="100%" stopColor="transparent" />
        </radialGradient>
        <radialGradient id="velion-circuit-ink-gradient">
          <stop offset="0%" stopColor="#ffffff" />
          <stop offset="66%" stopColor="#1a1a1a" />
          <stop offset="100%" stopColor="transparent" />
        </radialGradient>
        <linearGradient id="velion-circuit-core-gradient" x1="0" x2="1" y1="0" y2="1">
          <stop offset="0%" stopColor="#343434" />
          <stop offset="100%" stopColor="#101010" />
        </linearGradient>
        <filter id="velion-circuit-soft-shadow" x="-50%" y="-50%" width="200%" height="200%">
          <feDropShadow dx="1.4" dy="2" floodColor="#1a1a1a" floodOpacity="0.16" stdDeviation="1.4" />
        </filter>
      </defs>

      <g className="velion-circuit-diagram__paths">
        {paths.map((path) => (
          <path d={path.d} id={`velion-circuit-path-${path.id}`} key={path.id} pathLength="100">
            {animateLines ? (
              <animate
                attributeName="stroke-dashoffset"
                calcMode="spline"
                dur="1.35s"
                fill="freeze"
                from="100"
                keySplines="0.25 0.1 0.4 1"
                keyTimes="0; 1"
                to="0"
              />
            ) : null}
          </path>
        ))}
      </g>

      {animateSignals ? (
        <g className="velion-circuit-diagram__signals">
          {paths.map((path, index) => (
            <circle className={`velion-circuit-diagram__signal is-${path.signal}`} cx="0" cy="0" key={path.id} r="3.2">
              <animateMotion begin={`${index * 0.45}s`} dur={path.time} repeatCount="indefinite">
                <mpath href={`#velion-circuit-path-${path.id}`} />
              </animateMotion>
            </circle>
          ))}
        </g>
      ) : null}

      <g className="velion-circuit-diagram__core">
        <g className="velion-circuit-diagram__pins">
          <rect height="5" rx="0.7" width="2.4" x="88" y="35" />
          <rect height="5" rx="0.7" width="2.4" x="100" y="35" />
          <rect height="5" rx="0.7" width="2.4" x="112" y="35" />
          <rect height="5" rx="0.7" width="2.4" x="88" y="60" />
          <rect height="5" rx="0.7" width="2.4" x="100" y="60" />
          <rect height="5" rx="0.7" width="2.4" x="112" y="60" />
          <rect height="2.4" rx="0.7" width="5" x="75" y="44" />
          <rect height="2.4" rx="0.7" width="5" x="75" y="54" />
          <rect height="2.4" rx="0.7" width="5" x="120" y="44" />
          <rect height="2.4" rx="0.7" width="5" x="120" y="54" />
        </g>
        <rect filter="url(#velion-circuit-soft-shadow)" height="24" rx="2.4" width="44" x="78" y="38" />
        <text x="100" y="53.5">{text}</text>
      </g>
    </svg>
  );
}
