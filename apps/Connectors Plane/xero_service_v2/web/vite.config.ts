import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// The dev SPA proxies API calls to the BFF. Xero's registered callback reaches
// the BFF directly on port 5002; override the proxy with VITE_BFF_ORIGIN if needed.
const BFF_ORIGIN = process.env.VITE_BFF_ORIGIN ?? "http://localhost:5002";
const BFF_HOST = new URL(BFF_ORIGIN).hostname;
const LOCAL_BFF = BFF_HOST === "localhost" || BFF_HOST === "127.0.0.1";

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": {
        target: BFF_ORIGIN,
        changeOrigin: !LOCAL_BFF,
      },
    },
  },
  build: {
    target: "esnext",
    sourcemap: true,
  },
});
