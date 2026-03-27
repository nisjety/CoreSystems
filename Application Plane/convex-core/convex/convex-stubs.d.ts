declare module "convex/values" {
  export const v: any;
}

declare module "convex/server" {
  export const defineSchema: any;
  export const defineTable: any;
}

declare const process: {
  env: Record<string, string | undefined>;
};
