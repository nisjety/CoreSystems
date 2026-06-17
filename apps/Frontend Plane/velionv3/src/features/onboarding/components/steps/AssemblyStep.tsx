import { For, Show } from "solid-js";
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
			<h1>Setting up {props.organizationName || "s workspace"}.</h1>
			<p>
				Your workspace is being built from the website, the knwledge we
				have for {props.organizationName}, and the intergrations.
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
							classList={{
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
	connectorCount: number;
	organizationName: string;
	websitePages: number;
};

export function AssemblyStepVisual(props: AssemblyStepVisualProps) {
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
						<strong>{props.connectorCount}</strong>
						<span>sources started</span>
					</article>
					<article>
						<strong>{props.activePlan}</strong>
						<span>launch plan</span>
					</article>
				</div>
			</div>
		</div>
	);
}
