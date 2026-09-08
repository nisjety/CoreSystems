import { Footer } from "@/components/core/footer/Footer";
import { HomeClientShell } from "./HomeClientShell";
import { BrandLogosSection } from "./sections/BrandLogosSection";
import { FeaturesSection } from "./sections/FeaturesSection";
import { HeroSection } from "./sections/HeroSection";
import { LayerSection } from "./sections/layer-section";
import { PreFooterStatementSection } from "./sections/PreFooterStatementSection";
import { ProblemSection } from "./sections/ProblemSection";
import { ProductLoopSection } from "./sections/ProductLoopSection";
import { SensesSection } from "./sections/SensesSection";

/**
 * Server composition of the homepage. Browser behavior lives in
 * HomeClientShell, while section islands retain only the JavaScript they use.
 */
export function VerevonHome() {
	return (
		<HomeClientShell
			content={
				<>
					<BrandLogosSection />
					<ProblemSection />
					<ProductLoopSection />
					<SensesSection />
					<FeaturesSection />
					<LayerSection />
					<PreFooterStatementSection />
				</>
			}
			footer={<Footer />}
			hero={<HeroSection />}
		/>
	);
}

export default VerevonHome;
