# Unit economics: cheap distribution must still pay for delivery

3 November 2026 · planning model · NOK excluding VAT

The founder's “CAD” is interpreted here as **CAC: customer acquisition cost**. There are no verified commercial cohorts, prices, margins or retention inputs in the materials reviewed. All numbers below are adjustable scenarios, not a valuation or financial forecast.

## Definitions and accounting policy

- **ARPA:** net recurring monthly revenue per paying organization after recurring discounts and credits. Seats/invites are not organizations.
- **Recurring contribution:** ARPA less model/tool consumption, hosting/storage, allocated support/operations and recurring partner charges. Allocate production monitoring and delivery labour; do not call it free because the founder does it.
- **Fully loaded CAC:** attributable sales/marketing cash + founder/sales labour at a stated loaded rate + pre-sales engineering + failed-pilot net delivery cost + one-time acquisition commissions, divided by new production-paying organizations from the corresponding cohort. Include unsuccessful prospects and pilots. Align a sales cohort and its later paid conversions; a calendar month's spend divided by unrelated conversions is misleading.
- **Successful production onboarding:** delivery cost less one-time net onboarding fees, tracked outside CAC in this model. Successful pilot fees/costs and pilot credits belong in the same one-time delivery ledger. Failed-pilot fees offset failed-pilot delivery cost in CAC. No cost, fee or credit may appear twice.
- **Recurring partner share:** a recurring cost in contribution; a one-time introduction fee belongs in CAC. Record both only if the actual commercial agreement includes both.
- **Adjusted payback:** (CAC + max(0, onboarding cost − onboarding fee)) / monthly recurring contribution. This is a simple run-rate planning measure, not a churn-adjusted cash forecast. Positive onboarding surplus is ignored here to keep the acquisition gate conservative.
- **24-month contribution LTV:** expected contribution from a customer active in month one, capped at 24 months, plus onboarding fee minus onboarding cost. This horizon avoids pretending a tiny new cohort supports an infinite lifetime estimate. No expansion is assumed.
- **24-month contribution LTV:CAC:** the previous value / fully loaded CAC. This differs from commonly quoted uncapped LTV figures.

With monthly logo churn `c`, monthly recurring contribution `g`, horizon `H = 24`, net onboarding fee `F` and onboarding cost `I`:

```text
Expected active-month factor = [1 − (1 − c)^H] / c
                              = H when c = 0
LTV24 = g × active-month factor + F − I
LTV24:CAC = LTV24 / CAC
```

This assumes equal month-one contribution, constant logo churn, no price change, no expansion and no discounting. Actual usage growth, annual contracts, cancellations and collection timing require a cohort cash-flow model. Zero churn in the calculator means a 24-month cap, not infinite LTV. Payback is unavailable when recurring contribution is non-positive; LTV:CAC is unavailable when CAC is zero or unknown.

## Illustrative scenarios

| Input/output, NOK unless noted | Downside | Base | Upside |
| --- | ---: | ---: | ---: |
| Net monthly revenue | 4,500 | 7,500 | 11,000 |
| Monthly model/tools/infrastructure | 900 | 700 | 900 |
| Monthly support/operations | 1,200 | 800 | 600 |
| Monthly recurring partner charge | 450 | 0 | 1,100 |
| Monthly contribution | 1,950 | 6,000 | 8,400 |
| Contribution margin | 43.3% | 80.0% | 76.4% |
| Fully loaded CAC | 40,000 | 30,000 | 20,000 |
| Successful onboarding cost | 30,000 | 18,000 | 12,000 |
| Net onboarding fee | 5,000 | 10,000 | 10,000 |
| Monthly logo churn assumption | 3.0% | 1.0% | 0.5% |
| Adjusted payback, months | 33.3 | 6.3 | 2.6 |
| 24-month contribution LTV | 8,708 | 120,593 | 188,422 |
| LTV24:CAC | 0.22× | 4.02× | 9.42× |

The downside shows why low pricing plus bespoke work can destroy a seemingly inexpensive sales strategy. The upside is a sensitivity case, not a growth target.

### Base CAC example: three new production customers

```text
24,000 cash sales/marketing
+ 40 founder hours × 750 = 30,000
+ 12,000 pre-sales engineering
+ 18,000 failed-pilot net delivery cost
+ 6,000 one-time commissions
= 90,000 / 3 production customers = 30,000 CAC
```

Keep successful onboarding out of the above numerator because the payback/LTV formulas already include it. Record pilot revenue, delivery and credits before interpreting a real cohort. The calculator accepts the resulting CAC, not a claim that three customers have been won.

## Price has to fit measured customer value

Qualification example, all assumed: 120 comparable cases/month × 30 minutes saved / 60 × 650 NOK loaded hourly capacity cost × 60% realization = **23,400 NOK/month of capacity value**. At 7,500/month the ratio is **3.12×** before one-time costs.

Saved time is capacity, not automatically cash savings. Count the time needed to review, correct and handle errors. Demonstrate how freed capacity is used; do not count the same benefit again as avoided hires or additional margin. Measure actual reduction in customer losses or rework separately if supported by evidence.

At only 40 cases/month with the same assumptions, capacity value is 7,800 NOK/month, which barely covers a 7,500 subscription. Either find a higher-value job, reduce the cost/price or reject the fit.

## Proposed internal guardrails

- Aim for at least 70% recurring contribution margin after support and variable partner cost, and adjusted payback within 12 months. These are chosen management targets, not sourced market benchmarks.
- Use sensitivity ranges until paid renewal cohorts establish retention; do not optimize the headline ratio by assuming lower churn.
- Review customer concentration, cash collection and delivery capacity alongside LTV:CAC. Excellent ratios do not finance months of unpaid setup.
- Compare channels by fully loaded cost per 90-day retained paying organization and customer value. Early zero-conversion channels have costs and undefined CAC, not zero CAC.
- A marketplace, association session or referral is low-cash only when its preparation, fees and support burden are counted.

## How to improve the ratio in this strategy

| Lever | Mechanism | Evidence required |
| --- | --- | --- |
| Narrow ICP | Fewer irrelevant demos and lost pilots | Qualified-to-paid conversion by same-segment cohort |
| Same workflow and systems | Less implementation and support per account | Setup hours and recurring exceptions decline across customers |
| Adviser introductions | Less time reaching the right buyer | Net savings after enablement, fees and revenue share |
| Useful peer evidence | Less perceived risk and sales repetition | Reference-sourced paid conversions and cycle time |
| Team adoption | More sustained use and expansion | Retained workflow use and paid expansion; not merely more seats |
| Measured outcome | Better renewal and justified pricing | Buyer-accepted value, paid renewal and cost-to-serve |
| Cost-controlled execution | Preserve margin as usage grows | Actual per-case cost distribution and customer spending limits |

Use [the offline calculator](economics.html) to see which assumptions make the plan viable. Replace every scenario input with actual cohort data as it becomes available.
