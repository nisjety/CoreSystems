import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SearchPanel } from "@/features/dashboard-v2/components/VerevonHome";

type SearchPanelSnapshotState = ComponentProps<
	typeof SearchPanel
>["initialSnapshot"];

function jsonResponse(body: unknown, init: ResponseInit = {}) {
	return new Response(JSON.stringify(body), {
		status: init.status ?? 200,
		headers: { "Content-Type": "application/json" },
	});
}

function SearchPanelHarness() {
	const [expanded, setExpanded] = useState(false);
	const [snapshot, setSnapshot] =
		useState<SearchPanelSnapshotState>(null);

	return (
		<SearchPanel
			expanded={expanded}
			initialSnapshot={snapshot}
			onExpandedChange={setExpanded}
			onSnapshotChange={setSnapshot}
		/>
	);
}

describe("SearchPanel", () => {
	beforeEach(() => {
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
						mode: "search",
						results: [
							{
								url: "https://www.nhi.no/graviditet-og-fodsel/graviditet/uke-for-uke/7/",
								title: "Graviditetsorakelet foster Uke: 7 - NHI.no",
								snippet:
									"Embryo har fortsatt en hale, men den vil forsvinne i lopet av noen fa uker.",
							},
							{
								url: "https://www.helsenorge.no/gravid/gravid-uke-7/",
								title: "Gravid uke 7 - Helsenorge",
								snippet:
									"Gravid uke 7 Embryoet er vanligvis rundt 9 mm langt pa dette stadiet.",
							},
						],
						answer: "Verevon fant 2 relevante treff for \"7 uker embryo\".",
						citations: [],
					},
				});
			}

			if (url === "/api/v1/search/images") {
				return jsonResponse({ data: { images: [] } });
			}

			throw new Error(`Unexpected fetch: ${url}`);
		});
	});

	afterEach(() => {
		vi.restoreAllMocks();
	});

	it("shows preview results while typing without auto-expanding", async () => {
		const user = userEvent.setup();
		render(<SearchPanelHarness />);

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
		expect(
			screen.getByRole("button", { name: /se mer/i }),
		).toBeVisible();
	});

	it("expands into the full search shell when the user presses Enter", async () => {
		const user = userEvent.setup();
		render(<SearchPanelHarness />);

		await user.type(
			screen.getByRole("combobox", {
				name: "Søk i selskapets kunnskap",
			}),
			"7 uker embryo",
		);

		await waitFor(() =>
			expect(
				screen.getByRole("button", { name: /se mer/i }),
			).toBeVisible(),
		);
		await user.keyboard("{Enter}");

		await waitFor(
			() =>
				expect(
					screen.getByPlaceholderText("Skriv et nytt søk..."),
				).toBeVisible(),
			{ timeout: 2500 },
		);
		expect(
			screen.getByRole("button", { name: /kompakt/i }),
		).toBeVisible();
		expect(
			screen.getByDisplayValue("7 uker embryo"),
		).toBeVisible();
	});

	it("returns to compact mode when the expanded query is cleared", async () => {
		const user = userEvent.setup();
		render(<SearchPanelHarness />);

		await user.type(
			screen.getByRole("combobox", {
				name: "Søk i selskapets kunnskap",
			}),
			"7 uker embryo",
		);

		await waitFor(() =>
			expect(
				screen.getByRole("button", { name: /se mer/i }),
			).toBeVisible(),
		);
		await user.keyboard("{Enter}");

		const expandedInput = await screen.findByPlaceholderText(
			"Skriv et nytt søk...",
			{},
			{ timeout: 2500 },
		);
		await user.clear(expandedInput);

		await waitFor(() =>
			expect(screen.getByPlaceholderText("Ask anything…")).toBeVisible(),
		);
		expect(
			screen.queryByRole("button", { name: /kompakt/i }),
		).not.toBeInTheDocument();
	});
});
