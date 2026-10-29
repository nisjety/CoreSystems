/**
 * Convex Configuration
 * 
 * Configures Convex for local development with SQLite storage
 */

import { defineApp } from "convex/server";

export const app = defineApp({
  auth: {
    providers: [
      {
        domain: process.env.AUTH_SERVER_URL || "http://localhost:3011",
        applicationID: "convex-gateway",
      },
    ],
  },
});

export default app;
