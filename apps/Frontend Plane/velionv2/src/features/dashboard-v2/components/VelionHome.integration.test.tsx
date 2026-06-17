import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AnchorHTMLAttributes, ImgHTMLAttributes } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const navigationMocks = vi.hoisted(() => ({
	prefetch: vi.fn(),
	push: vi.fn(),
}));

vi.mock("next/image", () => ({
	default: ({
		alt,
		src,
		...props
	}: ImgHTMLAttributes<HTMLImageElement> & { src: string }) => (
		// eslint-disable-next-line @next/next/no-img-element
		<img alt={alt} src={src} {...props} />
	),
}));

vi.mock("next/link", () => ({
	default: ({
		children,
		href,
		...props
	}: AnchorHTMLAttributes<HTMLAnchorElement> & { href: string }) => (
		<a href={href} {...props}>
			{children}
		</a>
	),
}));

vi.mock("next/navigation", () => ({
	useRouter: () => ({
		prefetch: navigationMocks.prefetch,
		push: navigationMocks.push,
	}),
}));

vi.mock("@/features/composer-v2/components/DashboardComposer", () => ({
	DashboardComposer: () => <div>Dashboard composer</div>,
}));

vi.mock("@/features/dashboard-v2/components/VelionInformationCards", () => ({
	NewsDashboardCard: ({
		card,
		onPrompt,
	}: {
		card: { title: string };
		onPrompt: (card: { title: string }) => void;
	}) => (
		<button onClick={() => onPrompt(card)} type="button">
			{card.title}
		</button>
	),
	TrafficDashboardCard: ({
		card,
		onPrompt,
	}: {
		card: { title: string };
		onPrompt: (card: { title: string }) => void;
	}) => (
		<button onClick={() => onPrompt(card)} type="button">
			{card.title}
		</button>
	),
	WeatherDashboardCard: ({
		card,
		onPrompt,
	}: {
		card: { title: string };
		onPrompt: (card: { title: string }) => void;
	}) => (
		<button onClick={() => onPrompt(card)} type="button">
			{card.title}
		</button>
	),
}));

vi.mock("@/features/shell-v2/lib/control-plane-provider", () => ({
	useControlPlaneContext: () => ({
		entitlements: { plan: "free" },
		organization: { plan: "free" },
		user: { email: "test@example.com", name: "Test User" },
	}),
}));

import { VelionHome } from "@/features/dashboard-v2/components/VelionHome";

function jsonResponse(body: unknown, init: ResponseInit = {}) {
	return new Response(JSON.stringify(body), {
		status: init.status ?? 200,
		headers: { "Content-Type": "application/json" },
	});
}

describe("VelionHome search expansion", () => {
	beforeEach(() => {
		navigationMocks.prefetch.mockReset();
		navigationMocks.push.mockReset();

		vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
			const url =
				typeof input === "string"
					? input
					: input instanceof URL
						? input.toString()
						: input.url;

			if (url.startsWith("/api/v1/search/suggestions?")) {
				return jsonResponse({
					data: {
						suggestions: [
							{
								text: "7 uker embryo",
								source: "queries",
								collection: "queries",
								object: "7-uker-embryo",
							},
						],
					},
				});
			}

			if (url === "/api/v1/search/web") {
				return jsonResponse({
					data: {
						answer: "Velion fant 2 relevante treff for \"7 uker embryo\".",
						citations: [],
						mode: "search",
						results: [
							{
								snippet:
									"Embryo har fortsatt en hale, men den vil forsvinne i lopet av noen fa uker.",
								title: "Graviditetsorakelet foster Uke: 7 - NHI.no",
								url: "https://www.nhi.no/graviditet-og-fodsel/graviditet/uke-for-uke/7/",
							},
							{
								snippet:
									"Gravid uke 7 Embryoet er vanligvis rundt 9 mm langt pa dette stadiet.",
								title: "Gravid uke 7 - Helsenorge",
								url: "https://www.helsenorge.no/gravid/gravid-uke-7/",
							},
						],
					},
				});
			}

			if (url === "/api/v1/search/images") {
				return jsonResponse({ data: { images: [] } });
			}

			throw new Error(`Unexpected fetch: ${url}`);
		});
	});

	it("keeps previews compact while typing and expands only after Enter", async () => {
		const user = userEvent.setup();
		render(<VelionHome />);

		await user.click(screen.getByRole("button", { name: "Søk" }));
		await user.type(
			screen.getByRole("combobox", {
				name: "Søk i selskapets kunnskap",
			}),
			"7 uker embryo",
		);

		await waitFor(() =>
			expect(
				screen.getAllByText(
					"Graviditetsorakelet foster Uke: 7 - NHI.no",
				).length,
			).toBeGreaterThan(0),
		);
		expect(
			screen.getByPlaceholderText("Ask anything…"),
		).toBeVisible();
		expect(
			screen.queryByRole("button", { name: /kompakt/i }),
		).not.toBeInTheDocument();

		await user.keyboard("{Enter}");

		const expandedInput = await screen.findByPlaceholderText(
			"Skriv et nytt søk...",
			{},
			{ timeout: 2500 },
		);
		expect(expandedInput).toHaveValue("7 uker embryo");
		expect(screen.getByRole("button", { name: /kompakt/i })).toBeVisible();
		await waitFor(() =>
			expect(
				screen.getAllByText(
					"Graviditetsorakelet foster Uke: 7 - NHI.no",
				).length,
			).toBeGreaterThan(0),
		);
	});
});
