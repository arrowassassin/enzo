# Sweep 6 — AI × Health / Biotech / Science / Education

*Unbiased rerun, 2026-09-02. No founder-profile constraint. 26 searches before the session cap; only GitHub was fetchable. Practitioner sentiment is from secondary write-ups of Reddit/survey data.*

## Landscape signals (2025–2026)

**Capital is concentrating in a few "obvious" lanes.**
- Scribes: Abridge $300M Series E at $5.3B plus a $316M extension at the same valuation (Apr 2026) — flat-round signal ([MobiHealthNews](https://www.mobihealthnews.com/news/abridge-secures-300m-boosts-valuation-53b), [ValueAdd](https://valueaddvc.com/blog/abridge-valuation-2026-5-3b-100m-arr-and-how-the-ai-scribe-beat-nuance-and-ambience)).
- Point-of-care reference: OpenEvidence $250M Series D at $12B (Jan 2026), ~$300M annualized revenue ([CNBC](https://www.cnbc.com/2026/01/21/openevidence-chatgpt-for-doctors-doubles-valuation-to-12-billion.html)).
- Patient-facing agents: Hippocratic AI $126M Series C at $3.5B ([Fierce](https://www.fiercehealthcare.com/ai-and-machine-learning/hippocratic-ai-lands-126m-series-c-expand-patient-facing-ai-agents-fuel-ma)).
- Drug discovery: Isomorphic $2.1B Series B (May 2026); Chai ~$3.8B; Xaira ~$1.3B raised ([BioSpace](https://www.biospace.com/press-releases/isomorphic-labs-secures-2-1-billion-funding-to-scale-its-ai-drug-design-engine)).
- AI scientists: Periodic Labs $300M seed, seeking $500M at $7.5B ([Forbes](https://www.forbes.com/sites/iainmartin/2026/05/07/former-openai-researcher-to-raise-500-million-for-ai-science-startup/)).
- Edtech: MagicSchool ~$65M total, 6M educators; Brisk $15M, 1M teachers ([Tracxn](https://tracxn.com/d/companies/magicschool/__0LOioeIluO5KWsLD-7nbtWxu_Es-e11xO5QGuPrtJdg)).

**Labs moved into the generic layer.** ChatGPT Health (Jan 7, 2026) connects records/wearables; Claude for Healthcare (Jan 11) ships prior-auth, care-coordination and a CMS Coverage Database connector ([TechCrunch](https://techcrunch.com/2026/01/12/anthropic-announces-claude-for-healthcare-following-openais-chatgpt-health-reveal/)).

**Shutdowns define the graveyard.** Forward ($400M, $1M CarePods) — owning clinics kills unit economics ([Fierce](https://www.fiercehealthcare.com/health-tech/primary-care-player-forward-shutters-after-raising-400m-rolling-out-carepods)). Woebot exited consumer (Jun 2025). Kintsugi ($30M, 1,600-patient pivotal study) died waiting on FDA clearance; CEO called healthcare AI "financially unsustainable" for startups ([Forbes](https://www.forbes.com/sites/victordey/2026/02/17/kintsugi-ceo-says-building-ai-for-healthcare-is-financially-unsustainable-for-startups/)).

**Regulation is now a product spec.** Four states ban AI-delivered therapy (IL, NV, RI, ME); IL's WOPR explicitly *permits* AI for "administrative and supplementary support" for licensed clinicians ([IDFPR](https://idfpr.illinois.gov/news/2025/gov-pritzker-signs-state-leg-prohibiting-ai-therapy-in-il.html)). NYC schools bar AI for grading, discipline, and IEPs ([GovTech](https://www.govtech.com/education/k-12/nyc-schools-prohibit-ai-for-grading-discipline-ieps)). FDA list: 1,524 AI device authorizations (Aug 2026), 295 added in 2025, radiology 76% ([Imaging Wire](https://theimagingwire.com/2026/03/11/numbers-from-the-fda-show-radiology-is-maintaining-its-lead/)).

**Practitioner sentiment (secondary sources).** Physicians: 54% use AI; "five tools, drowning in subscriptions" ([DeepCura](https://www.deepcura.com/resources/best-ai-for-medical-professionals-reddit)). Nurses: NNU says the ecosystem is "designed to automate, de-skill and ultimately replace caregivers" ([AOL](https://www.aol.com/ai-nurses-staffing-solution-hospitals-130212301.html)). Professors: blue-book revival, oral exams, process portfolios replacing detection ([Axios](https://www.axios.com/2026/03/14/ai-blue-books-colleges-jobs)). Wet labs: OSS automation is thin — PyLabRobot 524 stars, MADSci 81, OpenSDL "alpha" — versus Sakana AI-Scientist at 14.5k stars. The software-side "AI scientist" is crowded; the hardware-execution side is empty ([PyLabRobot](https://github.com/PyLabRobot/pylabrobot), [AI-Scientist](https://github.com/SakanaAI/AI-Scientist)).

**Red zone (do not enter):** provider-side prior auth/denials (Cohere Health, Infinitus, Insurf, Claimglide, Clicks, Bookend, AKASA, Waystar, Candid, ExaCare) — and Claude for Healthcare ships a CMS-coverage connector.

## Ideas

Scores: Market / Timing / Lab-immunity / Whitespace / Moat / Capital-efficiency / Demand-evidence.

### 1. "Lab-as-API": protocol compilation, execution and verification layer for AI-scientist agents
**Pitch:** hardware-agnostic middleware that turns an agent's proposed experiment into a verified, auditable robot run across heterogeneous benchtop instruments — sold to pharma/materials R&D and to AI-scientist companies as their physical backend.
**Why now:** ~$3B+ poured into AI-scientist/drug-design in 12 months, all bottlenecked on physical validation; OSS execution layer is alpha-grade.
**Competitors:** Emerald Cloud Lab/Strateos-style cloud labs (heavy capex), PyLabRobot (OSS), instrument vendors' closed schedulers. Low crowdedness at the integration/verification layer.
**Lab-immunity:** Anthropic/OpenAI won't own liquid handlers, device drivers, or GLP audit trails.
**Scores:** 7 / 9 / 9 / 8 / 7 / 5 / 6.

### 2. Clearance-and-coverage engine for non-radiology AI diagnostics
**Pitch:** a regulatory+reimbursement company that licenses academic/open models (pathology/ophthalmology/derm/voice), runs the 510(k)/De Novo + PCCP path and builds the CPT/coverage dossier, then revenue-shares with health systems — "the Xaira of regulatory."
**Why now:** 1,524 authorizations, 76% radiology — other modalities under-served; Kintsugi shows a solo startup can't survive the timeline.
**Lab-immunity:** labs will not hold FDA clearances or negotiate payer coverage.
**Scores:** 8 / 7 / 10 / 7 / 8 / 4 / 6.

### 3. Compliance-attested behavioral-health copilot for the ban states
**Pitch:** AI that is legally *only* "administrative and supplementary support" under IL WOPR/NV/RI/ME — intake, measurement-based care, note drafting — with a state-specific audit log that proves no therapeutic decision-making by AI.
**Competitors:** Eleos, Blueprint, Upheal (therapy notes) — crowded on notes, empty on statutory attestation.
**Scores:** 6 / 8 / 7 / 5 / 6 / 8 / 6.

### 4. Assessed-by-voice: oral-defense and process-provenance platform for higher ed and secondary writing
**Pitch:** replaces AI detection with AI-conducted, rubric-scored viva-style defenses and cryptographically logged drafting process, integrated with the LMS and accepted by accreditation bodies.
**Why now:** blue books and oral exams are back but don't scale; detection tools distrusted; NYC bans AI-as-grader.
**Lab-immunity:** structurally adversarial to labs' incentive (ChatGPT Edu wants student usage).
**Scores:** 6 / 8 / 7 / 6 / 6 / 8 / 7.

### 5. District policy-as-code AI gateway for K-12
**Scores:** 5 / 7 / 6 / 5 / 6 / 8 / 5.

### 6. Post-acute and home-health operations AI (SNF, home health, hospice)
**Scores:** 7 / 6 / 6 / 6 / 5 / 7 / 4.

## Ranked shortlist
1. **Lab-as-API execution/verification layer** (best timing × immunity × whitespace).
2. **Clearance-and-coverage engine for non-radiology diagnostics** (highest immunity/moat; capital-heavy).
3. **Oral-defense / process-provenance assessment** (cheap, real demand, institutional moat).
4. **Ban-state behavioral-health copilot**.
5. **Post-acute ops AI**.
6. **K-12 policy gateway**.

## Verdict
The generic layers of this sector — scribes, clinical reference, patient chat, prior auth — are either saturated ($5B+ incumbents) or now bundled by OpenAI/Anthropic, and the shutdown record shows that owning clinics or waiting alone on FDA are the two fastest ways to die. Whitespace has migrated to the edges where models cannot go: physical experiment execution, regulatory clearance as a portfolio business, statute-specific compliance products, and assessment models that prove human work rather than detect machine work. The winning founders will look like a regulatory affairs lead plus a roboticist or an accreditation insider, not a prompt engineer.
