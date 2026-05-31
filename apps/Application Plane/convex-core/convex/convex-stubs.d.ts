declare module "convex/values" {
  export const v: any;
}

declare module "convex/server" {
  export const defineApp: any;
  export const defineSchema: any;
  export const defineTable: any;
  export const httpRouter: any;
}

declare const process: {
  env: Record<string, string | undefined>;
};
