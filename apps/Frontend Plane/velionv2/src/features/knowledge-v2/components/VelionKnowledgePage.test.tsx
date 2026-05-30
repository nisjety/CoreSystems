import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { VelionKnowledgePage } from "@/features/knowledge-v2/components/VelionKnowledgePage";

describe("VelionKnowledgePage", () => {
  it("opens on the folder and integration overview", () => {
    render(<VelionKnowledgePage />);

    expect(screen.getByRole("button", { name: /select knowledge collection/i })).toHaveTextContent("General Knowledge");
    expect(screen.getByRole("heading", { name: /^folders$/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /^integrations$/i, level: 2 })).toBeVisible();
    expect(screen.getByRole("heading", { name: /^files$/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /onboarding/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /google drive/i })).toBeVisible();
    expect(screen.getByText(/onboarding-guide\.pdf/i)).toBeVisible();
  });

  it("can switch from overview to graph and chunks", async () => {
    const user = userEvent.setup();
    render(<VelionKnowledgePage />);

    await user.click(screen.getByRole("button", { name: /graph/i }));

    expect(screen.getByRole("region", { name: /raggraph relationship map/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /shipping faq/i, level: 2 })).toBeVisible();

    await user.click(screen.getByRole("button", { name: /chunks/i }));

    expect(screen.getByRole("heading", { name: /^sources$/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /returns policy v4/i })).toBeVisible();
  });
});
