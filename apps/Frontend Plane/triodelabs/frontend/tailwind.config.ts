import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/features/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        editorial: {
          canvas: "#FCFCFA",
          surface: "#FFFFFF",
          ink: "#161616",
          secondary: "#59666B",
          muted: "#7B8589",
          rule: "#E5E9EA",
          dark: "#153845",
          ocean: "#91BDCA",
          oceanSoft: "#DCECF0",
          silver: "#E5E9EA",
        },
        ocean: { DEFAULT: "#91BDCA", soft: "#DCECF0", deep: "#153845" },
        silver: "#E5E9EA",
        // Extended Grays
        gray: {
          50: "#f9fafb",
          100: "#f3f4f6",
          200: "#e5e7eb",
          300: "#d1d5db",
          400: "#9ca3af",
          500: "#6b7280",
          600: "#4b5563",
          700: "#374151",
          800: "#1f2937",
          900: "#111827",
        },
      },
      // Typography
      fontFamily: {
        sans: ["Manrope", "system-ui", "sans-serif"],
      },
      fontSize: {
        "7xl": "4.5rem",
        "8xl": "6rem",
        "9xl": "8rem",
      },
      // Animations & Easing
      animation: {
        "fade-in": "fadeIn 0.5s ease-in-out",
        "slide-up": "slideUp 0.5s ease-out",
        "pulse-slow": "pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
      },
      keyframes: {
        fadeIn: {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
        slideUp: {
          "0%": { transform: "translateY(20px)", opacity: "0" },
          "100%": { transform: "translateY(0)", opacity: "1" },
        },
      },
      // Spacing
      spacing: {
        "18": "4.5rem",
        "22": "5.5rem",
        "88": "22rem",
        "128": "32rem",
      },
      // Border Radius
      borderRadius: {
        "4xl": "2rem",
        "5xl": "2.5rem",
      },
      // Shadows
      boxShadow: {
        soft: "0 4px 20px rgba(0, 0, 0, 0.08)",
        hover:
          "0 20px 40px -5px rgba(0, 0, 0, 0.15), 0 8px 16px -6px rgba(0, 0, 0, 0.1)",
        "hover-blue":
          "0 20px 25px -5px rgba(59, 130, 246, 0.1), 0 10px 10px -5px rgba(59, 130, 246, 0.04)",
        "hover-purple":
          "0 20px 40px -5px rgba(147, 51, 234, 0.25), 0 8px 16px -6px rgba(147, 51, 234, 0.15)",
      },
      // Backdrop Blur
      backdropBlur: {
        xs: "2px",
      },
    },
  },
  plugins: [],
  // Optimaliseringer for produksjon
  future: {
    hoverOnlyWhenSupported: true,
  },
  // Reduser CSS bundle størrelse
  corePlugins: {
    preflight: true,
    container: false, // Bruk custom container
  },
};
export default config;
