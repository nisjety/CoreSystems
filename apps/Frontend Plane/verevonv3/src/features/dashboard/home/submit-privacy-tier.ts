import { lastKnownAiModelPrivacyTier } from "@/shared/ai/model-selection";
import {
	groupChatModels,
	isVerevonModeId,
	listModels,
	type ModelInfo,
} from "@/shared/api/chat-client";
import {
	isSelectablePrivacyTier,
	type PrivacyTier,
} from "@/shared/api/privacy-tier";

// ── The privacy tier a composer turn carries ────────────────────────────────
//
// A catalogue model's tier rides every turn as `min_privacy_tier`, so the
// fallback chain can never route that turn to a weaker provider. The tier
// comes from `/api/v1/models`, and a turn sent before that catalogue had
// loaded used to go out with no tier at all: the loosest constraint, not the
// model's. These functions pick the tier without that gap.

/**
 * How long a send waits for a catalogue that is still loading. Long enough
 * for a normal page load, short enough that a slow gateway is not a hang.
 */
export const CATALOGUE_TIER_WAIT_MS = 3_000;

/**
 * The tier a turn carries when its model's own tier cannot be found out. The
 * strictest tier, because that is how every layer below this one handles
 * doubt: the gateway BFF passes an unknown tier on untouched instead of
 * guessing, model-gateway refuses an unknown tier number rather than treat it
 * as no constraint, and inference-core refuses a turn no provider can serve at
 * its tier (a typed error naming the tier), never a silent downgrade. A
 * sovereign floor either finds a provider that meets it or fails in the open.
 */
export const FAIL_CLOSED_PRIVACY_TIER: PrivacyTier = "sovereign";

/** Where a turn's tier came from. Tests and diagnostics read it. */
export type SubmitPrivacyTierSource =
	/** A Verevon intent mode (or no model): resolved server-side, no tier by design. */
	| "intent-mode"
	/** The loaded catalogue's own answer for the selected model. */
	| "catalogue"
	/** The tier the catalogue last reported for this exact model and provider. */
	| "last-known"
	/** Nothing known: the strictest tier. */
	| "fail-closed";

export type SubmitPrivacyTier = {
	/** The tier to send, or `undefined` for no constraint (key left off the payload). */
	tier: Exclude<PrivacyTier, "unspecified"> | undefined;
	source: SubmitPrivacyTierSource;
};

/** An intent mode, or no model at all (the backend reads `''` as Balance). */
export function isIntentModeSelection(model: string): boolean {
	const id = model.trim();
	return !id || isVerevonModeId(id);
}

/**
 * The tier a turn for this selection must carry.
 *
 * - An intent mode carries none: the backend picks the model, and the tier
 *   with it.
 * - Once the catalogue has loaded and lists the model, its tier is the answer,
 *   including "none" for a model that claims no tier. This is what a send has
 *   always done after loading.
 * - Otherwise (still loading, failed, or the model is missing from it), the
 *   last tier the catalogue reported for this same model and provider.
 * - Failing that, {@link FAIL_CLOSED_PRIVACY_TIER}.
 */
export function resolveSubmitPrivacyTier(input: {
	model: string;
	provider: string | undefined;
	/** `undefined` while the catalogue is unknown: still loading, or failed. */
	catalogue: readonly ModelInfo[] | undefined;
	/** From the stored selection, only when it names this model and provider. */
	lastKnownTier: PrivacyTier | undefined;
}): SubmitPrivacyTier {
	if (isIntentModeSelection(input.model))
		return { tier: undefined, source: "intent-mode" };
	const listed = input.catalogue
		? findCatalogueChatModel(input.catalogue, input.model, input.provider)
		: undefined;
	if (listed) return { tier: selectableTier(listed.privacyTier), source: "catalogue" };
	if (input.lastKnownTier)
		return { tier: selectableTier(input.lastKnownTier), source: "last-known" };
	return { tier: FAIL_CLOSED_PRIVACY_TIER as "sovereign", source: "fail-closed" };
}

/**
 * The tier to remember for a catalogue model. `'unspecified'` stands for "the
 * catalogue claims no tier", so a remembered answer can say "none" and still
 * be told apart from "never seen".
 */
export function rememberedPrivacyTier(model: ModelInfo): PrivacyTier {
	return model.privacyTier ?? "unspecified";
}

/**
 * The chat model the composer's picker would show for this id and provider.
 * Matches like the picker: an undefined provider only matches an undefined
 * provider.
 */
function findCatalogueChatModel(
	catalogue: readonly ModelInfo[],
	model: string,
	provider: string | undefined,
): ModelInfo | undefined {
	return groupChatModels(catalogue)
		.flatMap((group) => group.models)
		.find((candidate) => candidate.id === model && candidate.provider === provider);
}

function selectableTier(
	tier: PrivacyTier | undefined,
): Exclude<PrivacyTier, "unspecified"> | undefined {
	return isSelectablePrivacyTier(tier) ? tier : undefined;
}

/**
 * Resolves with the catalogue request, or after `ms` if it has not settled by
 * then. Never rejects, and clears its timer either way.
 */
export function waitForCatalogue<T>(request: Promise<T>, ms: number): Promise<void> {
	return new Promise<void>((resolve) => {
		const timer = setTimeout(resolve, ms);
		const done = () => {
			clearTimeout(timer);
			resolve();
		};
		request.then(done, done);
	});
}

// ── The same tier from the other surfaces ───────────────────────────────────
//
// The main composer puts the pieces above together itself, around the one
// catalogue request its own picker renders from. A surface that sends turns
// through the shared `AiModelPicker` (a Space room, Inbox assist) gets the
// same logic here, so a turn it sends carries the tier the main composer
// would send for the same selection.

export type TurnPrivacyTierResolver = {
	/**
	 * The catalogue request: started on the first call, shared after that.
	 * Hand it to the picker, so the picker and a send read one answer.
	 */
	catalogue(): Promise<ModelInfo[]>;
	/**
	 * {@link resolveSubmitPrivacyTier} for this selection, over the catalogue
	 * and the stored last-known tier. Waits up to
	 * {@link CATALOGUE_TIER_WAIT_MS} for a catalogue still in flight. Never
	 * waits for an intent mode or a settled catalogue, and never rejects.
	 */
	resolve(
		model: string,
		provider: string | undefined,
	): Promise<SubmitPrivacyTier>;
};

export function createTurnPrivacyTierResolver(options: {
	/** The org whose stored model selection holds the last-known tier. */
	orgId: string;
	/** The catalogue request. Defaults to `listModels`. */
	loadCatalogue?: () => Promise<ModelInfo[]>;
}): TurnPrivacyTierResolver {
	const load = options.loadCatalogue ?? listModels;
	let request: Promise<ModelInfo[]> | undefined;
	// `undefined` while the request is in flight (or not started), `null`
	// once it failed.
	let settled: ModelInfo[] | null | undefined;
	const catalogue = (): Promise<ModelInfo[]> => {
		if (!request) {
			let started: Promise<ModelInfo[]>;
			try {
				started = Promise.resolve(load());
			} catch (error) {
				started = Promise.reject(error);
			}
			request = started;
			// Registered first, so a send waiting on the request finds
			// `settled` already set when the wait ends.
			started.then(
				(list) => {
					settled = Array.isArray(list) ? list : null;
				},
				() => {
					settled = null;
				},
			);
		}
		return request;
	};
	return {
		catalogue,
		async resolve(model, provider) {
			if (!isIntentModeSelection(model) && settled === undefined)
				await waitForCatalogue(catalogue(), CATALOGUE_TIER_WAIT_MS);
			return resolveSubmitPrivacyTier({
				model,
				provider,
				catalogue: settled ?? undefined,
				lastKnownTier: lastKnownAiModelPrivacyTier(options.orgId, {
					model,
					provider,
				}),
			});
		},
	};
}
