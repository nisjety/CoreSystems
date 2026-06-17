'use client';

import React from 'react';

interface LiquidCornerProps {
  className?: string;
  onClick?: () => void;
}

export function LiquidCorner({ className = '', onClick }: LiquidCornerProps) {
  const [isHovering, setIsHovering] = React.useState(false);
  const motion = '650ms cubic-bezier(0.16, 1, 0.3, 1)';

  return (
    <svg
      className={className}
      viewBox="0 0 420 300"
      preserveAspectRatio="none"
      aria-hidden="true"
      onClick={onClick}
      onMouseEnter={() => setIsHovering(true)}
      onMouseLeave={() => setIsHovering(false)}
      style={{ cursor: onClick ? 'pointer' : 'default' }}
    >
      <defs>
        <linearGradient id="buttonGradient" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor={isHovering ? '#F5F5F5' : '#FFFFFF'} />
          <stop offset="100%" stopColor={isHovering ? '#EBEBEB' : '#FAFAFA'} />
        </linearGradient>
      </defs>

      <path
        d="M 0 260
           C 39 250, 52 200, 78 160
           C 98 120, 128 95, 170 90
           L 280 90
           C 335 90, 370 85, 395 70
           C 410 55, 420 20, 420 0
           L 420 270
           L 0 270
           Z"
        fill="white"
      />

      <g style={{ transition: 'opacity 200ms ease' }} opacity={isHovering ? 1 : 0.95}>
        <rect
          x="96"
          y="150"
          width="300"
          height="90"
          rx="45"
          fill="rgba(0, 0, 0, 0.04)"
          style={{ transition: `fill ${motion}` }}
        />

        <rect
          x="101"
          y="150"
          width="290"
          height="88"
          rx="44"
          fill="url(#buttonGradient)"
          stroke="#E8853D"
          strokeWidth="2"
          style={{ transition: `fill ${motion}` }}
        />
      </g>

      <g
        opacity={isHovering ? 1 : 0}
        transform={`translate(${isHovering ? 0 : -10}, 0)`}
        style={{ transition: `opacity ${motion}, transform ${motion}` }}
      >
        <path
          d="M 132 195 L 146 194 L 149 191.5 L 146 189 L 132 188"
          stroke="#E8853D"
          strokeWidth="1.8"
          fill="none"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </g>

      <text
        x={isHovering ? 248 : 243}
        y="210"
        textAnchor="middle"
        fontSize={isHovering ? '34' : '31'}
        fontWeight="700"
        letterSpacing="0.22em"
        fill="#E8853D"
        style={{
          pointerEvents: 'none',
          transition: `font-size ${motion}, letter-spacing ${motion}`
        }}
      >
        CHAT
      </text>

      <g
        opacity={isHovering ? 1 : 0}
        transform={`translate(${isHovering ? 0 : 10}, 0)`}
        style={{ transition: `opacity ${motion}, transform ${motion}` }}
      >
        <path
          d="M 356 195 L 342 194 L 339 191.5 L 342 189 L 356 188"
          stroke="#E8853D"
          strokeWidth="1.8"
          fill="none"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </g>
    </svg>
  );
}
