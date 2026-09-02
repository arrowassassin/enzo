# Deep dive 2 — Independent certification and compliance for physical AI

*2026-09-02. 41 searches; nearly every full-page fetch was proxy-blocked (only GitHub succeeded), so treat all claims as search-snippet-sourced unless marked [full page].*

## 1. Regulatory detail

**EU Cyber Resilience Act (Reg. 2024/2847)**
- **11 Sep 2026:** Art. 14 reporting live — actively exploited vulnerabilities and severe incidents; early warning 24h, notification 72h, final report 14 days (vulns) / 1 month (incidents), via the ENISA single reporting platform ([Element](https://www.element.com/resources/articles/cyber-resilience-act-article-14-reporting-obligations-guide), [Crowell](https://www.crowell.com/en/insights/client-alerts/eu-cyber-resilience-act-countdown-11-september-2026-incidentvulnerability-reporting-deadline-is-less-than-100-days-away), [EC](https://digital-strategy.ec.europa.eu/en/policies/cra-reporting)).
- **11 Dec 2027:** essential requirements, conformity assessment, CE marking, technical documentation, SBOM ([Anchore](https://anchore.com/sbom/eu-cra/), [Xygeni](https://xygeni.io/blog/cyber-resilience-act-timeline/)).
- **Scope:** manufacturers carry the main load; importers and distributors have verification duties; anyone who brands a third-party product inherits manufacturer duties ([Aegister](https://www.aegister.com/en/cms/insights/cyber-resilience-act-cra-obligations-manufacturers/)). Connected industrial machinery and robots are explicitly in scope ([Taylor Wessing](https://www.taylorwessing.com/en/insights-and-events/insights/2025/11/cyber-resilience-act-overview)).
- **Class I/II:** Class I may self-assess only if fully applying harmonised standards; Class II requires a notified body ([Tributech](https://www.tributech.io/blog/classify-iot-products-cyber-resilience-act), [BSI](https://www.bsi.bund.de/EN/Themen/Unternehmen-und-Organisationen/Informationen-und-Empfehlungen/Cyber_Resilience_Act/cyber_resilience_act_node.html)). Most robots are default-category (self-assessment) unless they embed a listed component.
- **Penalties:** up to €15M or 2.5% global turnover; enforceable 11 Dec 2027; micro/small firms exempt from fines for missing the 24h window ([White & Case](https://www.whitecase.com/insight-alert/cyber-resilience-act-clock-ticking-compliance)).
- **Notified-body gap:** CRA rules on notified bodies applied from 11 Jun 2026 but none were designated at that date ([cyberresilienceact.eu](https://www.cyberresilienceact.eu/news/cra-notified-bodies-rules-apply-11-june-2026.html)).

**Machinery Regulation 2023/1230** (applies 20 Jan 2027)
- **Annex I Part A** (mandatory third-party assessment, no module A): item 5 "safety components with fully or partially self-evolving behaviour using machine learning approaches ensuring safety functions", item 6 machinery embedding such systems ([Nemko](https://digital.nemko.com/regulations/eu-machinery-regulation), [regulations.ai](https://regulations.ai/regulations/RAI-EU-NA-MACHREG-2023)). Routes: EU-type examination (module B) + conformity to type (C), full QA (H), or unit verification (G). Part B categories may still self-certify under module A when harmonised standards fully apply.
- **Cybersecurity ESHR 1.1.9:** safety functions must be protected against accidental or malicious corruption ([CSA Group](https://www.csagroup.org/global-certification-regulatory-update/eu-adopts-new-machinery-regulation-eu-2023-1230-transition-and-surveillance-requirements/)).
- **Delay pressure:** industry associations asked to push cyber requirements to the CRA date and AI assessments two more years; ETUC opposed; **not adopted — 20 Jan 2027 remains binding** ([IBF](https://www.ibf-solutions.com/en/news-and-knowledge/technical-papers-and-news-on-ce-marking/postponement-of-mr-requirements-for-cybersecurity-and-ai), [CEN-CENELEC/ETUC](https://www.cencenelec.eu/news-events/news/2025/newsletter/ots-67-etuc/)).
- **Harmonised standards:** first MR citations expected Q3 2026, complete list targeted end-2026 ([Globalnorm](https://compliance.globalnorm.de/en/product-compliance-news/detail/harmonized-standards-for-the-machinery-regulation-eu-2023-1230/), [Intertek](https://www.intertek.com/blog/2026/02-09-eu-machinery-regulation-conformity/)). ISO 10218-1/-2:2025 published Feb 2025 ([IBF](https://www.ibf-solutions.com/en/seminars-and-news/news/new-standards-for-industrial-robots-en-iso-10218-1-and-2)); ISO 13482 at FDIS. **No harmonised standard exists for humanoids as a system** ([TÜV SÜD](https://www.tuvsud.com/en/services/testing/humanoid-robotics)).
- **AI-in-safety standards:** ISO/IEC TR 5469:2024 is only a technical report; ISO/IEC TS 22440 planned for 2026 ([IEC](https://www.iec.ch/blog/iec-and-iso-launch-working-group-advance-functional-safety-ai-systems)). Net: **no harmonised yardstick for an ML safety function** today, so Annex I Part A assessments will be bespoke NB judgment.
- **NB readiness:** TÜV SÜD first on NANDO (Sep 2024); Intertek designation pending Dec 2025; guides warn of a "severe shortage" ([TÜV SÜD](https://www.tuvsud.com/en/newsroom/press-releases/2024/september/tuev-sued-becomes-the-worlds-first-notified-body-for-the-new-machinery-regulation), [Intertek](https://www.intertek.com/news/2025/intertek-notified-body-accreditation-machinery-regulation-eu-2023-1230/)).

**AI Act / Digital Omnibus** — in force 27 Jul 2026. Annex III → 2 Dec 2027; Annex I embedded → 2 Aug 2028. **Machinery Regulation moved from Section A to Section B of Annex I**, so AI in machinery is governed by the MR, not the AI Act high-risk regime ([Gibson Dunn](https://www.gibsondunn.com/eu-ai-act-omnibus-agreement-postponed-high-risk-deadlines-and-other-key-changes/), [Cooley](https://cdp.cooley.com/digital-ai-omnibus-delays-key-deadlines-introduces-new-rules/)).

**US:** ANSI/A3 R15.06-2025 three-part standard (Oct 2025); R15.08 for IMRs ([Robot Report](https://www.therobotreport.com/now-available-full-403-page-ansi-a3-r15-06-2025-robot-safety-standard/)). UL 3300 added to OSHA's NRTL list; first public-facing robot cert to Simbe Tally, Mar 2026 ([UL](https://www.ul.com/news/ul-solutions-grants-its-first-global-certification-public-facing-robot-simbes-tally-autonomous)). UL 4600 accepts simulation + closed-course evidence ([Ansys](https://www.ansys.com/blog/ul4600-making-the-case)). **China:** MIIT Humanoid & Embodied Intelligence Standard System (2026) ([SESEC](https://sesec.eu/2026/04/01/chinas-first-standards-system-for-humanoid-robots-and-embodied-intelligence/)). **UK:** no CRA equivalent.

## 2. Competitive landscape

| Player | What they do (2026) | Funding | Gap |
|---|---|---|---|
| TÜV SÜD | First MR NB; humanoid testing page covers sensors, control, software, AI functions, cyber; Machinery Roadshow Nov 2026 ([TÜV SÜD](https://www.tuvsud.com/en/services/testing/humanoid-robotics)) | — | Consultancy-style, no policy-level metrics |
| TÜV Rheinland | Robot Integrator Program; CE'd Chery's AiMOGA humanoid ([TÜV](https://www.tuv.com/landingpage/en/robotics/)) | — | Same |
| UL Solutions | UL 3300 certs, UL 4600, Korea service-robot lab | — | US-centric; no learned-policy eval |
| Intertek | MR NB pending; MR/AI/cyber advisory | — | — |
| Pilz | SEM/SAM cyber assessments for MR+CRA ([Pilz](https://www.pilz.com/en-INT/support/law-standards-norms/manufacturer-machine-operators/machinery-regulation)) | — | Not an NB for AI |
| Fraunhofer IPA | Modular humanoid benchmark, 6 criteria incl. functional safety ([Fraunhofer](https://www.ipa.fraunhofer.de/en/press-media/press_releases/benchmark-for-humanoid-robots.html)) | public | **Closest to "UL for robot brains"** — research institute, not accredited certifier |
| Saphira AI (YC S24) | Safety-case/risk-assessment automation for ISO 10218/13482/61508/R15.08; customers 1X, Dexterity, RobCo ([YC](https://www.ycombinator.com/companies/saphira-ai)) | $500K seed | **Direct wedge competitor**, tiny |
| CRACI (Helsinki) | CI/CD-integrated CRA SBOM/vuln/docs ([TFN](https://techfundingnews.com/craci-cyber-resilience-act-compliance/)) | €1.4M pre-seed May 2026 | Software-generic, not machinery |
| SyncSoft, Inkog | Content-marketing MR/AI-Act checklists | undisclosed | Thin |
| Exein | Embedded runtime security; €170M raised 2025, M&A program 2026 ([SecurityWeek](https://www.securityweek.com/iot-security-firm-exein-raises-e100-million/)) | €170M+ | Will buy CRA tooling |
| Finite State / Manifest / Cybellum | Firmware SBOM; $72.8M / $15M A / LG $240M 2021 ([TechCrunch](https://techcrunch.com/2021/09/22/lg-is-acquiring-automotive-cybersecurity-startup-cybellum-in-a-240m-deal/)) | — | Cyber only; no safety/MR |
| Vanta/Drata | No CRA framework listed as of May 2026 ([Vanta](https://www.vanta.com/all-categories/compliance-frameworks)) | Vanta $4.15B val | Could add module anytime |
| Antioch | Sim-based robot testing, $8.5M seed Apr 2026 ([SiliconANGLE](https://siliconangle.com/2026/04/16/antioch-prepares-accelerate-simulated-testing-autonomous-robots-raising-8-5m/)) | $12.7M | Dev tool, not independent |
| Lightwheel | Co-built Isaac Lab-Arena; $145M B, >$600M val ([RoboticsIntl](https://www.roboticsintl.com/article/lightwheel-closes-145m-series-b-for-robotics-simulation-platform)) | $145M | Vendor-aligned sim, no real-world independence |
| XDOF / Foxglove | Teleop data ($70M) / observability ($40M B) | — | Data infra, not eval |

No funded startup found doing independent real-world policy evaluation for certification purposes. Academic groundwork: RoboArena (double-blind, 7 institutions, [arXiv](https://arxiv.org/abs/2506.18123)), RoboDojo (sim+real, expert teleoperators as baseline, [arXiv](https://arxiv.org/pdf/2607.04434)), RedVLA physical red-teaming ([arXiv](https://arxiv.org/pdf/2604.22591)), CoRL 2026 Physical AI Safety workshop ([spais-ws](https://spais-ws.org/)).

## 3. Market size (bottom-up)

- Global TIC ~$275B (2026); industrial/manufacturing ~13% ≈ $34B ([Fortune](https://www.fortunebusinessinsights.com/testing-inspection-certification-tic-market-104939)).
- VDMA: ~3,600 member firms; EU installed 67,800 industrial robots 2024; 121 AMR/AGV makers in EU ([VDMA](https://vdma.eu/en/mechanical-plant-engineering-figures-charts), [IFR](https://ifr.org/ifr-press-releases/news/global-robot-demand-in-factories-doubles-over-10-years)). CRA: >600K companies in scope worldwide ([Startup Reporter](https://www.startupreporter.eu/craci-cyber-resilience-act-compliance-seed-funding-finland/)).
- Price anchors: machinery CE €3.5–16.5K; module B €8–25K; NB hourly ~€317–325, 3–6 month queues ([ecocomply](https://ecocomply.ai/blog/what-does-ce-marking-cost-for-your-product), [cenitia](https://cenitia.com/library/when-you-need-a-notified-body)).
- Rough SAM: ~30K EU machinery/robot product families needing CRA docs × €5–20K/yr SaaS ≈ €150–600M; Annex I Part A AI assessments are a few hundred products/yr near-term × €50–150K.
- Insurers: Munich Re aiSure to ~$15M; HSB AI liability for SMEs (Mar 2026); humanoids flagged as emerging casualty risk; AXA/Allianz/Zurich have no dedicated products ([Reinsurance Business](https://www.insurancebusinessmag.com/reinsurance/news/breaking-news/munich-re-tech-radar-flags-ai-cyber-and-climate-as-top-strategic-themes-for-reinsurers-571977.aspx), [agentinsured](https://agentinsured.eu/articles/axa-allianz-zurich-tier1-european-insurer-ai-stance-2026)). Demand is latent, not contracted.

## 4. Willingness to pay

Real but concentrated in cyber: law-firm countdown alerts; Lenovo "Sr. Manager, CRA Compliance" posting Jul 2026 ([LinkedIn](https://www.linkedin.com/jobs/view/sr-manager-cyber-resilience-act-compliance-at-lenovo-4437923805)); CRACI funded on the deadline narrative. Robot OEMs are hiring safety leads — Agility "Principal Functional Safety Engineer", Meta humanoid "System Safety Engineer" ([Agility](https://jobs.industrialinnovationfund.amazon/companies/agility-robotics/jobs/55998033-principal-functional-safety-engineer), [Meta](https://www.metacareers.com/profile/job_details/968490272833104)) — building in-house rather than buying. Saphira's 1X/Dexterity logos prove OEMs pay for safety-case tooling. No public Amazon/GXO/DHL RFP language found requiring third-party policy evidence.

## 5. Test-lab feasibility

- Method: double-blind pairwise real-world trials (RoboArena), sim+real with expert-teleop ceilings (RoboDojo), intervention-rate logging, physical red-teaming (RedVLA). Isaac Lab-Arena (v0.2 alpha, Apache-2.0) provides parallel sim eval but **no real-world validation** [full page: [GitHub](https://github.com/isaac-sim/IsaacLab-Arena)].
- Regulatory acceptance: UL 4600 explicitly accepts simulation+closed-course evidence; MR/NBs have no stated position on sim-only evidence for ML safety functions.
- Accreditation: ISO/IEC 17025 from scratch 12–24 months ([Care Europe](https://care-europe.com/blog/iso-17025-certification-cost-timeline-process/)). NB designation is a multi-year, national-authority process.
- Floor cost (estimate): 500–1,000 m², 3–6 robot cells, mocap, force plates, dummies ≈ €1.5–3M capex plus €2–3M/yr opex.

## 6. Business model options

1. **Compliance SaaS per product family:** €6–24K/yr (CRA SBOM/vuln/ENISA reporting + MR technical file + risk file).
2. **Audit/pre-assessment as a service:** €25–80K per Annex I Part A product, feeding TÜV/Intertek.
3. **Evidence pack per policy release:** €15–50K per policy version (autonomy rate, intervention rate, failure taxonomy, safety envelope).
4. **Insurer partnership:** rated evidence pack as underwriting input.
5. **NB partnership first, own designation later.**

## 7. Team and capital

To $1M ARR (SaaS wedge): 2 founders + 1 ex-NB machinery/functional-safety assessor (IEC 61508/ISO 13849), 1 product-security/SBOM engineer, 2 full-stack, 1 seller; €2–3M seed, 18 months, ~60 customers at €15K. To accredited lab: add 1 robot-learning eval lead, 2 test engineers, a 17025 quality manager; €8–12M cumulative, 30–36 months.

## 8. Kill risks

- **Deadline slip:** MR delay was refused so far, but industry lobbied for it and the Omnibus already moved AI Act dates twice.
- **Module A self-certification:** only ML *safety functions* trigger Part A, and OEMs will architect around it (safety PLC wrapper).
- **No yardstick:** TS 22440 not yet published; humanoid standard absent; NBs will improvise, which favors incumbents.
- **Incumbents move:** TÜV SÜD's humanoid page and Nov 2026 roadshow; Exein's M&A war chest; Vanta can ship a CRA module in a quarter.
- **Tiny volumes:** 19,100 humanoid shipments H1 2026, 97% Chinese — a China-first standards regime.
- **NVIDIA/Lightwheel** own sim eval; a lab's differentiation must be real-world independence.

## 9. Comparables

TIC trades ~12x EV/EBITDA (Intertek Jul 2026); 18–23% margins ([Aventis](https://aventis-advisors.com/tic-company-valuations/)). Software: Cybellum $240M (LG), Robust Intelligence $400M (Cisco), Drata bought SafeBase $250M, Vanta $4.15B, Exein €170M raised, Lightwheel >$600M.

## Verdict

1. As framed (EU-first, SaaS + lab), this is a **strong lifestyle/services business with a venture-scale option**, not a clear venture bet: CE-style deadlines make budgets real, but per-product pricing and NB-gated demand cap growth.
2. The **CRA-for-connected-machinery SaaS is the wedge** — deadline is 9 days away, no incumbent GRC tool covers it, and machinery OEMs need SBOM + ENISA reporting + MR technical-file continuity in one tool.
3. The **lab is the venture upside** but is 2–3 years and €10M away, with no harmonised yardstick yet; run it as a paid "evidence pack" service via a TÜV/Intertek partnership before pursuing accreditation.
4. Saphira (safety-case tooling, YC, $500K) and Fraunhofer IPA are the two closest threats/partners; neither is funded to own the category.
5. Kill signal: an MR cybersecurity/AI postponement in 2027 or a Vanta/Exein CRA module before Dec 2027 — pivot to insurer-facing evidence packs if either lands.
