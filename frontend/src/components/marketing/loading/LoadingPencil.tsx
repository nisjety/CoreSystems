'use client';

import React from 'react';

interface LoadingPencilProps {
  rotateStroke?: boolean;
}

export default function LoadingPencil({ rotateStroke = false }: LoadingPencilProps) {
  return (
    <div className="pointer-events-none fixed inset-0 z-0 overflow-hidden">
      <div className={`absolute inset-0 ${rotateStroke ? 'pencil-darken' : ''}`} />
      <svg
        className={`absolute h-full w-full opacity-95 ${rotateStroke ? 'pencil-rotate' : ''}`}
        preserveAspectRatio="none"
        viewBox="0 0 200 600"
        style={{ width: '100%', height: '100%' }}
      >
        <defs>
          <linearGradient id="pencil-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#0F0F0F" stopOpacity="0.99" />
            <stop offset="65%" stopColor="#111111" stopOpacity="0.9" />
            <stop offset="100%" stopColor="#1A1A1A" stopOpacity="0.72" />
          </linearGradient>
          <linearGradient id="pencil-core" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#0A0A0A" stopOpacity="0.85" />
            <stop offset="75%" stopColor="#0F0F0F" stopOpacity="0.45" />
            <stop offset="100%" stopColor="#0F0F0F" stopOpacity="0.18" />
          </linearGradient>
          <mask id="pencil-reveal">
            <path
              d="M60 -24 C52 26, 147 56, 136 118 C124 174, 59 176, 63.5 226 C69 262, 103.5 260, 101.5 306 C99.5 382, 99.5 470, 103 624"
              fill="none"
              stroke="white"
              strokeWidth="30"
              strokeLinecap="round"
              strokeLinejoin="round"
              pathLength={1}
              className="reveal-path"
            />
          </mask>
        </defs>

        <path
          d="M50 -24 C42 26, 150 56, 140 118 C127 174, 60 176, 68 226 C74 262, 106 260, 104 306 C102 382, 98 470, 103.5 624 L102.5 624 C97 470, 101 382, 99 306 C97 260, 65 262, 59 226 C51 176, 118 174, 131 118 C141 56, 44 26, 70 -24 Z"
          fill="url(#pencil-fill)"
          mask="url(#pencil-reveal)"
        />
        <path
          d="M62 -24 C55 24, 142 58, 133 118 C122 173, 63 176, 67 226 C72 262, 101 261, 100 306 C99 380, 99 470, 102.5 624"
          fill="none"
          stroke="url(#pencil-core)"
          strokeWidth="2.3"
          strokeLinecap="round"
          strokeLinejoin="round"
          mask="url(#pencil-reveal)"
          className="core-line"
          vectorEffect="non-scaling-stroke"
        />
        <path
          d="M51 -24 C44 26, 149 56, 140 118 C127 174, 60 176, 68 226 C74 262, 106 260, 104 306 C102 382, 98 470, 103.5 624"
          fill="none"
          stroke="#090909"
          strokeOpacity="0.4"
          strokeWidth="0.9"
          strokeLinecap="round"
          strokeLinejoin="round"
          mask="url(#pencil-reveal)"
          className="edge-line"
          vectorEffect="non-scaling-stroke"
        />
      </svg>

      <style jsx>{`
        .reveal-path {
          stroke-dasharray: 1;
          stroke-dashoffset: 1;
          animation: drawStroke 2.05s cubic-bezier(0.42, 0, 0.58, 1) forwards;
        }

        .core-line,
        .edge-line {
          stroke-dasharray: 1;
          stroke-dashoffset: 1;
          animation: drawStroke 2.05s cubic-bezier(0.42, 0, 0.58, 1) forwards;
        }

        @keyframes drawStroke {
          to {
            stroke-dashoffset: 0;
          }
        }

        .pencil-rotate {
          transform-origin: 50% 50%;
          animation: pencilRotateZoom 1.2s cubic-bezier(0.18, 0.84, 0.22, 1) 2.05s forwards;
          will-change: transform;
        }

        .pencil-darken {
          background: #111111;
          opacity: 0;
          animation: darkenToBlack 1.2s ease-out 2.05s forwards;
        }

        @keyframes pencilRotateZoom {
          from {
            transform: rotate(0deg) scale(1);
          }
          to {
            transform: rotate(-90deg) scale(60);
          }
        }

        @keyframes darkenToBlack {
          from {
            opacity: 0;
          }
          to {
            opacity: 1;
          }
        }
      `}</style>
    </div>
  );
}
