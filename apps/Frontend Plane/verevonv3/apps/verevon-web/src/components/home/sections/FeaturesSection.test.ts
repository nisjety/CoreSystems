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

	it("names the six modules in Finn → Forstå → Få gjort order", () => {
		expect(moduleCards).toHaveLength(6);
		expect(moduleCards.map((card) => card.module)).toEqual([
			"Verevon Knowledge",
			"Verevon Research",
			"Verevon Chat",
			"Verevon Agents",
			"Verevon Support",
			"Verevon Trust",
		]);
		expect(moduleCards.map((card) => card.beat)).toEqual([
			"FINN",
			"FINN · UTENFOR",
			"FORSTÅ",
			"FÅ GJORT",
			"FÅ GJORT · KUNDER",
			"KONTROLL",
		]);
	});

	it("never shows the same beat label on two cards", () => {
		// A carousel that repeats "FINN" then "FINN" (or "FÅ GJORT" twice)
		// back to back reads as a bug, not as intentional stage grouping —
		// each card's beat text must be unique even when it shares a promise
		// stage with another card.
		const beats = moduleCards.map((card) => card.beat);
		expect(new Set(beats).size).toBe(beats.length);
	});

	it("gives every module its own photograph", () => {
		const images = moduleCards.map((card) => card.image);
		expect(new Set(images).size).toBe(moduleCards.length);
		images.forEach((image) => {
			expect(image).toMatch(/^\/verevon-mood\/module-[a-z0-9-]+\.jpg$/);
		});
	});

	it("never markets a Verevon Cloud module", () => {
		// Verevon runs on Azure and is not sold as infrastructure — naming a
		// "Cloud" module would overclaim exactly what the trust docs warn about.
		expect(
			moduleCards.some((card) => /cloud/i.test(card.module)),
		).toBe(false);
	});
});
