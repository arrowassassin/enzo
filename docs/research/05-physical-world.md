# Sweep 5 — AI × the Physical World

*Unbiased rerun, 2026-09-02. No founder-profile constraint. 32 searches before the session cap; only GitHub was fetchable. Reddit/X sentiment inferred from secondary coverage.*

## 1. State of the sector (evidence)

**Capital is flooding the top of the stack.** Robotics startups raised $18.8B in 2026 YTD vs $15B for all of 2025 ([Crunchbase News](https://news.crunchbase.com/robotics/startup-venture-funding-surges-2026-data/)). Figure $1B+ Series C at ~$39B ([Forge](https://forgeglobal.com/insights/figure-ai-robotics-growth-2026/)); Skild AI $1.4B at $14B+ ([BusinessWire](https://www.businesswire.com/news/home/20260114335623/en/Skild-AI-Raises-$1.4B-Now-Valued-Over-$14B)); Physical Intelligence $600M Series B at $5.6B ([Robot Report](https://www.therobotreport.com/physical-intelligence-raises-600m-advance-robot-foundation-models/)); Apptronik $520M at $5B ([CNBC](https://www.cnbc.com/2026/02/11/apptronik-raises-520-million-at-5-billion-valuation-for-apollo-robot.html)); THEKER $85M ([Sifted](https://sifted.eu/articles/theker-industrial-robotics-85m-series-a)); XDOF $70M for robot training data ([SiliconANGLE](https://siliconangle.com/2026/06/17/robotic-teleoperation-data-startup-xdof-launches-70m-funding/)). Defense: Saronic $1.75B at $9.25B ([CNBC](https://www.cnbc.com/2026/03/31/autonomous-boat-startup-saronic-raises-1point75-billion-.html)); Anduril $5B Series H at $61B ([Sacra](https://sacra.com/c/anduril/)); Scout AI $100M Series A ([AI Business](https://aibusiness.com/robotics/scout-ai-raises-100m-build-ai-brain-autonomous-warfare)).

**Reality gap.** As of July 2026 no verified customer delivery of a 1X NEO existed; 1X estimates only 60–70% autonomy in 2026 with teleop filling the rest ([RoboZaps](https://blog.robozaps.com/b/1x-neo-review)). IEEE Spectrum: "hard to rationalize what's actually happened… with the amount of money and hype" ([IEEE Spectrum](https://spectrum.ieee.org/top-robotics-stories-2025)). Chinese press reports the "first wave of shutdowns" among humanoid companies ([36kr](https://eu.36kr.com/en/p/3657117189890433)). READY Robotics ($41.5M raised) shut down; customers preferred integrated solutions from hardware OEMs over pure software ([robotics.press](https://robotics.press/news/ready-robotics-shuts-down-after-raising-41-5m-in/)). Picnic (pizza robots, $53M) liquidated May 2026 ([TheStreet](https://www.thestreet.com/technology/popular-robotics-company-shuts-down-and-liquidates-all-assets)).

**Model layer is commoditizing.** π0/π0.5 weights are Apache-2.0 ([openpi](https://github.com/Physical-Intelligence/openpi)); NVIDIA GR00T N1.7 ships under a commercial-use license ([Isaac-GR00T](https://github.com/NVIDIA/Isaac-GR00T)). Crunchbase notes tools, fleet management, embodied software and simulation "under-index on capital share".

**Regulatory catalysts (hard dates).** EU CRA vulnerability/incident reporting (24h early warning, 72h notification) goes live **11 Sept 2026** ([Freshfields](https://www.freshfields.com/en/our-thinking/blogs/technology-quotient/cyber-resilience-act-reporting-obligations-take-effect-on-11-september-2026-102nzmk)). EU Machinery Regulation 2023/1230 applies **20 Jan 2027**: AI performing a safety function makes machinery high-risk, pulling it into third-party conformity assessment ([Nemko](https://digital.nemko.com/regulations/eu-machinery-regulation), [Timelex](https://www.timelex.eu/en/blog/navigating-legal-maze-ai-autonomous-robots-and-eus-regulatory-overhaul)). FAA Part 108 (BVLOS) final rule most likely late 2026–2027; only 657 BVLOS waivers issued to date ([Airdata](https://airdata.com/blog/2026/part-108)). US federal counter-UAS spend ≥$1.8B in 2026 plus $500M in state/local grants ([Airsight](https://www.airsight.com/blog/anti-drone-market-2026-federal-funding)).

**Labor and grid.** 2.6M skilled-trades deficit, 0.6 entrants per retiree ([FieldCamp](https://fieldcamp.ai/ai-field-service-management-software/blog/field-service-trends/)); Microsoft's president called the electrician shortage "the number one problem" for datacenter expansion ([TradeColleges](https://tradecolleges.org/blog/skilled-trades-outlook/ai-physical-imperative-trades)); Lowe's ($250M) and BlackRock ($100M) are funding trades training ([Fortune](https://fortune.com/2026/04/07/lowes-major-investment-skilled-trades-training-electricans-plumbers-carpenters-ceo-marvin-ellison-critical-to-the-future-fortune-exclusive/)). 2,060 GW sits in US interconnection queues ([Quartz](https://qz.com/us-power-grid-ai-data-center-demand-constraints-051326)). Liquid cooling is consolidating fast (Ecolab–CoolIT $4.75B) ([Memoori](https://memoori.com/liquid-cooling-ma-deals-reshaping-data-centers-in-2026/)).

**Warehouse AMRs.** ~4.7M robots in 50K facilities; failures trace to WMS/ERP integration and non-linear fleet scaling ([SVRC](https://www.roboticscenter.ai/applications/warehouse-robotics)).

## 2. Startup ideas

### Idea A — Independent test & certification lab for physical-AI policies ("UL for robot brains")
**Pitch:** Third-party facility + software that measures a robot policy's real autonomy rate, failure modes and safety envelope, and produces evidence packs for buyers, insurers and EU notified bodies.
**Why now:** Machinery Regulation forces third-party conformity assessment for AI safety components on 20 Jan 2027; buyers are burned by teleop-inflated demos.
**Competitors:** TÜV/UL/Pilz (consulting, slow, not AI-native); XDOF/Cortex sell data to labs, not independent certification. Low crowding.
**Lab immunity:** Independence *is* the product. **Moat:** Accreditation, benchmark corpus, incident database, notified-body relationships.
**Scores:** Market 7 · Timing 9 · Lab-immunity 10 · Whitespace 8 · Moat 8 · Capital 4 · Demand 6.

### Idea B — Compliance & incident-reporting platform for connected machinery (CRA + Machinery Reg + AI Act)
**Pitch:** "Vanta for physical products": SBOM, risk files, technical documentation, and automated 24h/72h ENISA reporting for robots, drones, HVAC controllers, ag equipment.
**Why now:** Reporting obligations bite 11 Sept 2026, full CRA Dec 2027, Machinery Reg Jan 2027.
**Competitors:** Generic GRC (Vanta, Drata), SBOM tools, a few new entrants (SyncSoft, Inkog). Thin at the machinery layer.
**Scores:** Market 7 · Timing 10 · Lab-immunity 8 · Whitespace 6 · Moat 6 · Capital 9 · Demand 7.

### Idea C — Multi-vendor robot field-service and uptime network
**Pitch:** Third-party maintenance, spares and remote-triage network for mixed AMR/cobot/humanoid fleets, sold as an uptime SLA.
**Why now:** 4.7M robots across 50K sites, failing on integration and scaling; OEMs are racing volume without service infrastructure; READY's death shows pure orchestration software is unsellable — service is the wedge.
**Scores:** Market 8 · Timing 7 · Lab-immunity 10 · Whitespace 7 · Moat 7 · Capital 5 · Demand 6.

### Idea D — Counter-UAS detection and evidence platform for state/local buyers
**Why now:** DHS drone office, $500M grant pool, UK prisons competition. **Competitors:** Dedrone (Axon), DroneShield, D-Fend, Fortem, Anduril; crowded at DoD tier; underserved at SLED price points.
**Scores:** Market 7 · Timing 8 · Lab-immunity 9 · Whitespace 4 · Moat 5 · Capital 5 · Demand 8.

### Idea E — Trades diagnostic copilot with tool-data capture
**Why now:** 2.6M deficit; contractors already pay $245–398/tech/month to ServiceTitan ([Augmented Trades](https://augmentedtrades.com/best-ai-tools-for-hvac-contractors-2026/)). **Lab immunity:** weakest of the set — a general model does much of the reasoning.
**Scores:** Market 8 · Timing 8 · Lab-immunity 5 · Whitespace 5 · Moat 5 · Capital 9 · Demand 7.

### Idea F — Part 108 BVLOS operations-compliance stack (non-DJI)
**Scores:** Market 6 · Timing 6 · Lab-immunity 8 · Whitespace 3 · Moat 4 · Capital 8 · Demand 7.

### Anti-examples (checked, not recommended)
AI weather (DeepMind/NVIDIA can absorb the core); robot foundation models / generic teleop data (funded, open weights commoditize); autonomous labs/materials (Lila $552M, Periodic $300M own it); autonomous trucking (Aurora guides only $14–16M 2026 revenue).

## 3. Ranked shortlist
1. **A — Independent physical-AI test/certification lab**: highest lab-immunity and whitespace; hardest to start.
2. **B — CRA/Machinery-Reg compliance platform**: best capital efficiency and timing; pair with A as a software wedge.
3. **C — Vendor-neutral robot fleet service network**: boring, physical, defensible.
4. **D — SLED counter-UAS**.
5. **E — Trades diagnostic copilot** (lowest lab-immunity).
6. **F — Part 108 stack**.

## 4. Verdict
Money is piling into the model and humanoid layer while the layers that turn robots into working deployments — verification, compliance, service, and sensing for public-sector buyers — are under-capitalized and dated to hard regulatory triggers in the next 5–16 months. The most defensible opportunities are ones where independence, accreditation, or physical labor is the product, which no lab can ship as a feature. Build the compliance-and-certification wedge first (B→A), and treat the fleet-service network (C) as the second act once the humanoid installed base is real.
