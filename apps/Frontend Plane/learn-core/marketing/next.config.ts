import type { NextConfig } from "next";
import path from "node:path";

const config: NextConfig = {
  poweredByHeader: false,
  turbopack: { root: path.resolve(process.cwd(), "..") },
};
export default config;
