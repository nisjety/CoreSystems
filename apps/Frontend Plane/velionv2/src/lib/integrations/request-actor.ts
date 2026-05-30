import { getCurrentAuthUser } from "@/lib/auth/onboarding-access";

export type RequestActor = {
  userId: string;
  email?: string;
  name?: string;
  avatar?: string;
  cookieHeader?: string;
};

export class RequestActorError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export async function requireRequestActor(): Promise<RequestActor> {
  const user = await getCurrentAuthUser();

  if (!user?.id) {
    throw new RequestActorError(401, "authentication_required", "Sign in is required.");
  }

  return {
    userId: user.id,
    email: user.email,
    name: user.name,
    avatar: user.image ?? undefined,
    cookieHeader: user.cookieHeader,
  };
}
