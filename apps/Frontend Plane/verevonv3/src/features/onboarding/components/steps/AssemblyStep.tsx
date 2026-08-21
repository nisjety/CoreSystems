import { createMemo, For, Show } from "solid-js";
import type { PlanRecommendation } from "@/features/onboarding/lib/api";
import { Button } from "@/shared/ui/Button";

type AssemblyStepContentProps = {
	assemblyTicks: number;
	error?: string;
	onFinish: () => void;
	organizationName: string;
};

export function AssemblyStepContent(props: AssemblyStepContentProps) {
	return (
		<section class="onboarding-copy">
			<p class="onboarding-eyebrow">Final step · Assembly</p>
			<h1>Setting up {props.organizationName || "your workspace"}.</h1>
			<p>
				Your workspace is being built from the website, the knowledge we
				have for {props.organizationName || "your organization"}, and the
				integrations you connected.
			</p>

			<ul class="onboarding-assembly-list">
				<For
					each={[
						"Website grounded",
						"Organization created",
						"Connect sessions prepared",
						"Plan committed",
					]}
				>
					{(item, index) => (
						<li
							class={{
								"onboarding-assembly-list__item--done":
									index() < props.assemblyTicks,
							}}
						>
							<span>
								{index() < props.assemblyTicks ? "✓" : "·"}
							</span>
							{item}
						</li>
					)}
				</For>
			</ul>

			<Show when={props.error}>
				{(message) => (
					<p class="onboarding-error" role="alert">
						{message()}
					</p>
				)}
			</Show>

			<Button variant="primary" size="sm" onClick={props.onFinish}>
				Åpne dashboard
			</Button>
		</section>
	);
}

type AssemblyStepVisualProps = {
	activePlan: string;
	connectedSourceCount: number;
	organizationName: string;
	websitePages: number;
	recommendation?: PlanRecommendation;
};

export function AssemblyStepVisual(props: AssemblyStepVisualProps) {
	// Fuller recommendation detail, relocated here from the (non-scrolling)
	// paywall so that step stays concise. The user isn't making a decision on
	// this "workspace getting ready" screen, so there's room to show more.
	const scopeSignals = createMemo(
		() => props.recommendation?.scopeSignals?.filter(Boolean).slice(0, 5) ?? [],
	);
	const opportunities = createMemo(
		() => props.recommendation?.opportunities?.filter(Boolean).slice(0, 3) ?? [],
	);
	// Proof points beyond the two already shown on the paywall.
	const proofRest = createMemo(
		() => props.recommendation?.proofPoints?.filter(Boolean).slice(2) ?? [],
	);

	return (
		<div class="onboarding-right-surface onboarding-right-surface--summary">
			<div class="onboarding-summary-card">
				<div class="onboarding-summary-card__header">
					<span>First launch outlook</span>
				</div>
				<h2>
					{props.organizationName || "Your workspace"} is ready for
					the first operator.
				</h2>
				<p>
					The onboarding route used the new Rust action gateway for
					crawl preview, org creation, connect sessions, graph
					preview, and plan recommendation.
				</p>
				<div class="onboarding-summary-card__facts">
					<article>
						<strong>{props.websitePages || 1}</strong>
						<span>pages grounded</span>
					</article>
					<article>
						<strong>{props.connectedSourceCount}</strong>
						<span>sources started</span>
					</article>
					<article>
						<strong>{props.activePlan}</strong>
						<span>launch plan</span>
					</article>
				</div>

				<Show when={opportunities().length || scopeSignals().length || proofRest().length}>
					<div class="onboarding-summary-card__plan">
						<Show when={opportunities().length}>
							<div class="onboarding-summary-card__plan-block">
								<span class="onboarding-summary-card__plan-label">Første forbedringer</span>
								<ul>
									<For each={opportunities()}>{(item) => <li>{item}</li>}</For>
								</ul>
							</div>
						</Show>
						<Show when={proofRest().length}>
							<div class="onboarding-summary-card__plan-block">
								<span class="onboarding-summary-card__plan-label">Hvorfor {props.activePlan}</span>
								<ul>
									<For each={proofRest()}>{(item) => <li>{item}</li>}</For>
								</ul>
							</div>
						</Show>
						<Show when={scopeSignals().length}>
							<div class="onboarding-summary-card__plan-chips">
								<For each={scopeSignals()}>{(item) => <span>{item}</span>}</For>
							</div>
						</Show>
					</div>
				</Show>
			</div>
		</div>
	);
}
