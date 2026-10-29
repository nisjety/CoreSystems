"use client";

export default function AccentFlow() {
  return (
    <svg
      className="absolute inset-0 h-full w-full pointer-events-none overflow-visible"
      viewBox="0 0 1000 560"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id="flowGradientCore" x1="500" y1="280" x2="350" y2="700" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#FFF8D8" />
          <stop offset="10%" stopColor="#FFF0C0" />
          <stop offset="40%" stopColor="#FF6B3D" />
          <stop offset="100%" stopColor="#FF4D31" />
        </linearGradient>

        <linearGradient id="flowGradientBloom" x1="500" y1="280" x2="350" y2="700" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#FFE6C9" stopOpacity="0.9" />
          <stop offset="20%" stopColor="#FF5E3B" stopOpacity="0.8" />
          <stop offset="100%" stopColor="#FF2E1F" stopOpacity="0" />
        </linearGradient>

        <filter id="flowGlowSoft" x="-100%" y="-100%" width="400%" height="400%">
          <feGaussianBlur stdDeviation="8" result="blur" />
          <feComposite in="SourceGraphic" in2="blur" operator="over" />
        </filter>

        <filter id="flowGlowHot" x="-100%" y="-100%" width="400%" height="400%">
          <feGaussianBlur stdDeviation="2" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {/* Main Bloom path */}
      <path
        d="M 460 750 
           C 200 650, 250 500, 340 460 
           S 700 420, 600 380 
           S 500 320, 500 280"
        stroke="url(#flowGradientBloom)"
        strokeWidth="20"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
        filter="url(#flowGlowSoft)"
        opacity="0.8"
      />

      {/* Hot Core path */}
      <path
        d="M 460 750 
           C 200 650, 250 500, 340 460 
           S 700 420, 600 380 
           S 500 320, 500 280"
        stroke="url(#flowGradientCore)"
        strokeWidth="6"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
        filter="url(#flowGlowHot)"
        opacity="1"
      />

      {/* Animation tracer */}
      <path
        d="M 460 750 
           C 200 650, 250 500, 340 460 
           S 700 420, 600 380 
           S 500 320, 500 280"
        stroke="#FFF"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
        strokeDasharray="1000"
        strokeDashoffset="1000"
        className="animate-flow opacity-60 mix-blend-overlay"
      />
    </svg>
  );
}
