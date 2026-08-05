"use client";

import { motion, useReducedMotion } from "motion/react";

type Testimonial = {
	name: string;
	role: string;
	text: string;
};

const testimonials: Testimonial[] = [
	{
		text: "This ERP revolutionized our operations, streamlining finance and inventory. The cloud-based platform keeps us productive, even remotely.",
		name: "Briana Patton",
		role: "Operations Manager",
	},
	{
		text: "Implementing this ERP was smooth and quick. The customizable, user-friendly interface made team training effortless.",
		name: "Bilal Ahmed",
		role: "IT Manager",
	},
	{
		text: "The support team is exceptional, guiding us through setup and providing ongoing assistance, ensuring our satisfaction.",
		name: "Saman Malik",
		role: "Customer Support Lead",
	},
	{
		text: "This ERP's seamless integration enhanced our business operations and efficiency. Highly recommend for its intuitive interface.",
		name: "Omar Raza",
		role: "CEO",
	},
	{
		text: "Its robust features and quick support have transformed our workflow, making us significantly more efficient.",
		name: "Zainab Hussain",
		role: "Project Manager",
	},
	{
		text: "The smooth implementation exceeded expectations. It streamlined processes, improving overall business performance.",
		name: "Aliza Khan",
		role: "Business Analyst",
	},
	{
		text: "Our business functions improved with a user-friendly design and positive customer feedback.",
		name: "Farhan Siddiqui",
		role: "Marketing Director",
	},
	{
		text: "They delivered a solution that exceeded expectations, understanding our needs and enhancing our operations.",
		name: "Sana Sheikh",
		role: "Sales Manager",
	},
	{
		text: "Using this ERP, our online presence and conversions significantly improved, boosting business performance.",
		name: "Hassan Ali",
		role: "E-commerce Manager",
	},
];

const testimonialColumns = [
	testimonials.slice(0, 3),
	testimonials.slice(3, 6),
	testimonials.slice(6, 9),
];

function getInitials(name: string) {
	return name
		.split(" ")
		.map((part) => part[0] ?? "")
		.join("")
		.slice(0, 2)
		.toUpperCase();
}

function TestimonialCard({
	testimonial,
}: {
	testimonial: Testimonial;
}) {
	return (
		<article className="relative w-full overflow-hidden rounded-[8px] border border-white/34 bg-white/[0.78] p-7 text-verevon-j-text shadow-[0_28px_90px_rgba(21,24,26,0.16)] backdrop-blur-[18px]">
			<div
				aria-hidden="true"
				className="absolute inset-x-0 top-0 h-px bg-[linear-gradient(90deg,transparent,rgba(238,122,80,0.68),transparent)]"
			/>

			<p className="m-0 font-protokoll text-[0.98rem] font-light leading-7 text-verevon-j-text/78">
				“{testimonial.text}”
			</p>

			<div className="mt-6 flex items-center gap-4">
				<div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full border border-verevon-j-text/10 bg-[linear-gradient(135deg,color-mix(in_srgb,var(--verevon-bg-soft)_72%,white),color-mix(in_srgb,var(--verevon-a-earth)_18%,white))] font-protokoll text-sm font-medium uppercase tracking-[0.18em] text-verevon-j-text/74">
					{getInitials(testimonial.name)}
				</div>

				<div className="min-w-0">
					<p className="m-0 font-arbeit text-[1.02rem] font-medium leading-tight text-verevon-j-text">
						{testimonial.name}
					</p>
					<p className="m-0 mt-1 font-protokoll text-[0.85rem] uppercase tracking-[0.12em] text-verevon-j-text/48">
						{testimonial.role}
					</p>
				</div>
			</div>
		</article>
	);
}

function TestimonialsColumn({
	className = "",
	duration = 16,
	items,
	revealDelay = 0,
	shouldReduceMotion,
}: {
	items: Testimonial[];
	shouldReduceMotion: boolean;
	className?: string;
	duration?: number;
	revealDelay?: number;
}) {
	const repeatedItems = shouldReduceMotion ? items : [...items, ...items];

	return (
		<motion.div
			className={[
				"relative w-full max-w-[360px] overflow-hidden",
				className,
			]
				.filter(Boolean)
				.join(" ")}
			initial={shouldReduceMotion ? false : { y: 54 }}
			transition={{
				duration: 0.78,
				delay: revealDelay,
				ease: [0.16, 1, 0.3, 1],
			}}
			viewport={{ once: true, amount: 0.18 }}
			whileInView={shouldReduceMotion ? undefined : { y: 0 }}
		>
			<motion.div
				animate={shouldReduceMotion ? undefined : { y: "-50%" }}
				className="flex flex-col gap-6 pb-6"
				transition={
					shouldReduceMotion
						? undefined
						: {
								duration,
								ease: "linear",
								repeat: Infinity,
								repeatType: "loop",
							}
				}
			>
				{repeatedItems.map((testimonial, itemIndex) => (
					<TestimonialCard
						key={`${testimonial.name}-${testimonial.role}-${itemIndex}`}
						testimonial={testimonial}
					/>
				))}
			</motion.div>
		</motion.div>
	);
}

export function Testimonials() {
	const shouldReduceMotion = useReducedMotion() ?? false;

	return (
		<section
			className="relative z-[2] -mt-[100svh] overflow-hidden text-verevon-j-text max-[899px]:mt-0 max-[899px]:bg-verevon-j-text max-[899px]:text-verevon-c-white"
			id="testimonials"
		>
			<div className="relative z-10 mx-auto min-h-[140svh] max-w-[1440px] px-[clamp(24px,4vw,56px)] pb-[clamp(96px,10vw,156px)] pt-[104svh] max-[899px]:min-h-0 max-[899px]:py-[clamp(96px,10vw,156px)]">
				<div
					className="mx-auto flex max-w-[640px] flex-col items-center text-center"
				>
					<motion.div
						className="inline-flex rounded-full border border-verevon-j-text/14 bg-white/58 px-4 py-1.5 font-protokoll text-[0.82rem] uppercase tracking-[0.14em] text-verevon-j-text/64 shadow-[0_18px_60px_rgba(21,24,26,0.08)] backdrop-blur-[10px] max-[899px]:border-white/22 max-[899px]:bg-white/10 max-[899px]:text-white/72"
						initial={
							shouldReduceMotion
								? false
								: { clipPath: "inset(0 100% 0 0)" }
						}
						transition={{
							duration: 0.7,
							ease: [0.16, 1, 0.3, 1],
						}}
						viewport={{ once: true, amount: 0.7 }}
						whileInView={
							shouldReduceMotion
								? undefined
								: { clipPath: "inset(0 0% 0 0)" }
						}
					>
						Testimonials
					</motion.div>

					<motion.h2
						className="m-0 mt-5 font-arbeit text-[clamp(2.45rem,4vw,4.8rem)] font-light leading-[0.94] tracking-[-0.06em] text-verevon-j-text max-[899px]:text-white"
						initial={
							shouldReduceMotion
								? false
								: { clipPath: "inset(0 0 100% 0)", y: 26 }
						}
						transition={{
							duration: 0.9,
							delay: 0.08,
							ease: [0.16, 1, 0.3, 1],
						}}
						viewport={{ once: true, amount: 0.7 }}
						whileInView={
							shouldReduceMotion
								? undefined
								: { clipPath: "inset(0 0 0% 0)", y: 0 }
						}
					>
						What our users say
					</motion.h2>
					<motion.p
						className="m-0 mt-5 max-w-[560px] font-protokoll text-[clamp(1rem,1.05vw,1.16rem)] font-light leading-[1.6] text-verevon-j-text/64 max-[899px]:text-white/72"
						initial={
							shouldReduceMotion
								? false
								: { clipPath: "inset(0 0 100% 0)", y: 18 }
						}
						transition={{
							duration: 0.78,
							delay: 0.18,
							ease: [0.16, 1, 0.3, 1],
						}}
						viewport={{ once: true, amount: 0.7 }}
						whileInView={
							shouldReduceMotion
								? undefined
								: { clipPath: "inset(0 0 0% 0)", y: 0 }
						}
					>
						See what our customers have to say about the speed, clarity, and
						control they get from the workflow.
					</motion.p>
				</div>

				<div
					className="relative mx-auto mt-[clamp(42px,4.8vw,76px)] flex max-h-[740px] justify-center gap-6 overflow-hidden"
					data-testimonial-cards=""
				>
					<TestimonialsColumn
						duration={16}
						items={testimonialColumns[0]}
						revealDelay={0.08}
						shouldReduceMotion={shouldReduceMotion}
					/>
					<TestimonialsColumn
						className="hidden md:block"
						duration={20}
						items={testimonialColumns[1]}
						revealDelay={0.18}
						shouldReduceMotion={shouldReduceMotion}
					/>
					<TestimonialsColumn
						className="hidden xl:block"
						duration={18}
						items={testimonialColumns[2]}
						revealDelay={0.28}
						shouldReduceMotion={shouldReduceMotion}
					/>
				</div>
			</div>
		</section>
	);
}

export default Testimonials;
