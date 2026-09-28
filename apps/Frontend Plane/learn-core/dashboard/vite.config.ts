import { defineConfig } from "vite";
import solid from "@solidjs/vite-plugin";

export default defineConfig({
  plugins: [solid()],
  server: {
    host: "127.0.0.1",
    strictPort: true,
    proxy: {
      "/api/auth": {
        target: process.env.LEARNING_AUTH_URL ?? "http://127.0.0.1:3011",
        changeOrigin: false,
      },
      "/api/v1/learning": {
        target: "http://127.0.0.1:3196",
        changeOrigin: false,
      },
    },
  },
});
