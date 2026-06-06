import { describe, expect, it } from "vitest";
import { buildNavbarSeedFromControlPlaneContext } from "@/features/shell-v2/lib/navbar-server";

describe("buildNavbarSeedFromControlPlaneContext", () => {
  it("hydrates profile and theme without loading remote navbar panels", () => {
    const payload = buildNavbarSeedFromControlPlaneContext({
      user: {
        id: "user_123",
        email: "ima@example.com",
        image: "https://example.com/avatar.png",
        name: "Ima Fernandes",
      },
      appearance: {
        theme: "dark",
        colorScheme: "#123456",
      },
    });

    expect(payload.profile).toEqual({
      id: "user_123",
      name: "Ima Fernandes",
      email: "ima@example.com",
      avatar: "https://example.com/avatar.png",
      status: "online",
    });
    expect(payload.theme).toEqual({
      colorScheme: "#123456",
      theme: "dark",
    });
    expect(payload.calendar).toEqual({ events: [], notes: [] });
    expect(payload.notifications).toEqual({
      configured: false,
      unreadCount: 0,
      notifications: [],
      messages: [],
    });
  });
});
