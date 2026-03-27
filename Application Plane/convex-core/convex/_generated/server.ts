/* Auto-stubbed Convex server types for local typechecking. */

export type HandlerCtx = any;

export type QueryBuilder = (q: any) => any;

export type HandlerConfig<Args, Return> = {
  args: Args;
  handler: (ctx: HandlerCtx, args: any) => Promise<Return> | Return;
};

export function query<Args, Return>(config: HandlerConfig<Args, Return>) {
  return config as unknown as (args: Args) => Promise<Return>;
}

export function mutation<Args, Return>(config: HandlerConfig<Args, Return>) {
  return config as unknown as (args: Args) => Promise<Return>;
}

export function action<Args, Return>(config: HandlerConfig<Args, Return>) {
  return config as unknown as (args: Args) => Promise<Return>;
}

export function internalQuery<Args, Return>(config: HandlerConfig<Args, Return>) {
  return config as unknown as (args: Args) => Promise<Return>;
}

export function internalMutation<Args, Return>(config: HandlerConfig<Args, Return>) {
  return config as unknown as (args: Args) => Promise<Return>;
}

export function internalAction<Args, Return>(
  configOrHandler:
    | HandlerConfig<Args, Return>
    | ((ctx: HandlerCtx, args: Args) => Promise<Return> | Return),
) {
  return configOrHandler as unknown as (args: Args) => Promise<Return>;
}

export function httpAction(handler: (ctx: HandlerCtx, request: Request) => Promise<Response> | Response) {
  return handler;
}
