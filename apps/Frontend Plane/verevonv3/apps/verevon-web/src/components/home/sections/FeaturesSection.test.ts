import { describe, expect, it } from "vitest";
import { moduleCards, workflowCards } from "./feature-workflow-cards";

describe("features module cards", () => {
	it("keeps the four-step workflow sequence for the pinned section", () => {
		expect(workflowCards).toHaveLength(4);
		expect(workflowCards.map((card) => card.film)).toEqual([
			"build",
			"connect",
			"ground",
			"approve",
		]);
	});

	it("organizes five areas a first-time visitor can recognize", () => {
		expect(moduleCards).toHaveLength(5);
		expect(moduleCards.map((card) => card.area)).toEqual([
			"Kunnskap",
			"AI og agenter",
			"Verktøy for arbeidet",
			"Datamaskiner og support",
			"Tilgang og kontroll",
		]);
	});

	it("links each area to an existing relevant depth page", () => {
		expect(moduleCards.map((card) => card.href)).toEqual([
			"/plattform/felles-kontekst",
			"/produkt/arbeidsflyten",
			"/produkt/arbeidsflyten",
			"/produkt/svartid",
			"/plattform/kontrollert-arbeid",
		]);
		expect(moduleCards.every((card) => card.href.startsWith("/"))).toBe(true);
	});

	it("uses a distinct workplace photo for every area", () => {
		const images = moduleCards.map((card) => card.image);
		expect(new Set(images).size).toBe(moduleCards.length);
		expect(images.every((image) => image.startsWith("https://images.pexels.com/") || image.startsWith("https://images.unsplash.com/"))).toBe(true);
		expect(moduleCards.every((card) => Math.max(card.imageWidth, card.imageHeight) >= 3840)).toBe(true);
		expect(moduleCards.every((card) => card.imageAlt.length > 0)).toBe(true);
	});
});
