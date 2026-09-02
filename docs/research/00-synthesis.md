# AI startup whitespace — cross-sector synthesis and ranking (unbiased rerun)

*Date: 2026-09-02. Ten parallel research sweeps (eight sectors, one social-sentiment sweep across Reddit/X/HN/Product Hunt/Indie Hackers, one investor-thesis and macro sweep), each run with live web search where the session budget allowed. Sector reports 01–10 in this folder carry the sources. The first pass, which wrongly assumed a Rust/systems founder, is archived in `archive-v1-founder-biased/`.*

## The brief

Find an AI-domain project with real startup potential. Any sector: security, entertainment, gaming, utility, physical world, health, education, enterprise, infra. The product must not be something Anthropic, OpenAI, Google or Meta can erase with a model update or a feature release. Prefer whitespace: things outside what companies are already doing, but in the AI space. No constraint on team, stack or skills; the founding team is assumed to be assembled to fit the idea.

## The one-paragraph answer

The whitespace in September 2026 is **verification, not generation**. As generation becomes free, the scarce good becomes proof: proof that a person made this, proof that this claim photo is real, proof that this robot is safe, proof that this agent was allowed to do that. Six of the ten sweeps converged, from different sectors, on one opportunity: **human-provenance and AI-disclosure infrastructure**, a cross-platform registry and process-provenance layer that lets creators, students, studios and enterprises prove human authorship and satisfy the EU Article 50 obligations that went live on 2 August 2026. The labs are structurally the thing being certified against, so they cannot run the registry. Three more trust businesses fill out the top five: independent certification and compliance for physical AI (hard EU dates on 11 September 2026 and 20 January 2027), an eldercare agent authorised to act for aging parents, and scam-victim intervention for banks. The first pass's top pick, a safety runtime for coding agents, drops to the bottom third once founder-fit is removed: that lane drew $3.6B in 2026 and Docker, NVIDIA, Cloudflare and the labs are all shipping sandboxes.

## How the candidates were scored

Every sweep scored its own ideas on the same seven criteria. Twelve cross-sector candidates (merged where sweeps described the same business from different angles) were re-scored on one rubric. Lab-immunity is weighted double because it is the brief's hard constraint; evidence is weighted one and a half because independent convergence is the strongest signal this research produced. Founder fit is not a criterion.

| Criterion | Weight | What a 10 means |
|---|---|---|
| Lab-immunity | ×2 | Structurally impossible for a frontier lab to absorb with a release |
| Evidence | ×1.5 | Strength and independence of demand signals: funding, incidents, sentiment, accelerator requests, number of sweeps that surfaced it |
| Market | ×1 | Large paying market 2026–28 |
| Timing | ×1 | A catalyst is live now |
| Whitespace | ×1 | Nobody is doing it |
| Moat | ×1 | Durable advantage once built |
| Capital | ×1 | 10 = cheap to start |

Maximum score is 85.

## Ranked contenders

| # | Idea | Lab | Evid | Mkt | Time | White | Moat | Cap | **Score** | Sweeps that surfaced it |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **Human-provenance and AI-disclosure infrastructure** — cross-platform verified-human registry, capture-time process provenance for creators and students, studio disclosure audit trail, Article 50 deployer compliance | 9 | 10 | 7 | 10 | 7 | 7 | 8 | **72** | Entertainment, Gaming, Security, Health/Edu, Sentiment, Investor (YC "Proving You're Human") |
| 2 | **Independent certification and compliance for physical AI** — CRA and Machinery Regulation compliance software as the wedge, then an accredited test lab that measures real autonomy and safety envelopes for robots | 10 | 7 | 7 | 9 | 8 | 8 | 6 | **68.5** | Physical world, Health (clearance engine), Investor (Bessemer) |
| 3 | **Eldercare "family chief of staff"** — an agent with power-of-attorney scope that reads mail and portals, calls Medicare and insurers, coordinates siblings, and screens scam calls against enrolled family voices | 8 | 9 | 9 | 8 | 8 | 7 | 5 | **66.5** | Consumer, Security, Sentiment, Investor (YC "AI for Aging") |
| 4 | **Scam-victim intervention platform for banks and exchanges** — detect customers being scammed from behavioural and on-chain signals, run structured interventions, produce evidence packs for reimbursement | 9 | 8 | 8 | 8 | 7 | 7 | 5 | **65** | Security, Sentiment, Consumer |
| 5 | **Evidence-grade claims capture and AI-fraud screening for insurers** — sensor-signed capture plus forensic screening of every claim photo and document | 9 | 7 | 8 | 8 | 6 | 7 | 6 | **63.5** | Security (a paying vertical of #1) |
| 6 | **AI-native services firm in a licensed slow market** — own the accounting, brokerage or compliance firm and sell the completed work | 10 | 8 | 9 | 8 | 4 | 7 | 3 | **63** | Enterprise, Investor (Sequoia, YC, GC Creation), Sentiment |
| 7 | **Vertical RL-environment studio with expert graders** — training environments and human reward models for one regulated vertical, sold to labs | 8 | 7 | 8 | 9 | 6 | 7 | 6 | **62.5** | Infra, Investor (YC "Data for the Real World") |
| 8 | **Consent and licensing infrastructure for creative AI** — indie-artist licensing collective, licensed-IP fan platforms, consented voice-replica exchange | 8 | 8 | 7 | 8 | 6 | 8 | 5 | **62** | Entertainment, Gaming, Security |
| 9 | **Lab-as-API execution layer for AI-scientist agents** — hardware-agnostic middleware that turns a proposed experiment into a verified robot run | 9 | 5 | 7 | 9 | 8 | 7 | 5 | **61.5** | Health/Science |
| 10 | **Neutral multi-silicon inference certification** — tested serving builds certified per non-NVIDIA chip for sovereign and neocloud buyers | 9 | 6 | 8 | 8 | 7 | 6 | 4 | **60** | Infra |
| 11 | **Undo, receipts and identity for agents** — rollback and signed action logs for any agent | 6 | 9 | 8 | 9 | 4 | 5 | 8 | **59.5** | Sentiment, Infra (first-pass top pick) |
| 12 | **Private-credit portfolio-monitoring network** — covenant recomputation and borrower data pipe for mid-size credit managers | 7 | 5 | 7 | 8 | 6 | 8 | 6 | **56.5** | Enterprise (thinly researched) |

## Why #1: human-provenance and AI-disclosure infrastructure

**The problem, from six directions.** Deezer reports more than half of daily uploads are AI and up to 85% of streams on AI tracks were fraudulent; Spotify deleted 75 million tracks and launched Verified badges; iHeart banned AI from its air under a "Guaranteed Human" mark; the Authors Guild sells a $10 human-authored seal; film has a Human Made Mark. On Steam, 30.8% of 2026 releases carry an AI disclosure and those games take a fraction of sales, so studios now face publisher anti-AI clauses, awards disqualifications and SAG-AFTRA consent rules with no audit tooling. In education, Princeton scrapped its honour code, Denmark mandated oral defences, and the year's loudest student thread (23.7k upvotes) was a false AI accusation. Insurers say 99% have seen AI-altered claim documents. And since 2 August 2026, EU Article 50 obliges deployers to disclose and machine-mark synthetic content, with the marking grace period ending 2 December 2026.

**The gap.** Every response is siloed: one badge per platform, one seal per guild, one mark per industry, and nothing at all for the individual who needs to prove they did the work. There is no cross-platform registry, no capture-time process provenance for a student or an indie musician, no pipeline-integrated audit trail for a studio, and no deployer-side tool that checks whether a C2PA mark survives a company's own CMS and CDN.

**Why the labs cannot kill it.** They are the party being certified against. A lab-run human-authorship registry has no credibility, and SynthID and OpenAI's C2PA marks cover only each lab's own outputs. Every model release increases the demand for proof.

**Moat.** Registry network effects (a badge is worth more the more platforms honour it), platform and pipeline integrations, standards-body position, and a growing corpus of provenance evidence that becomes the de facto standard in disputes.

**Honest risks.** Consumer badge revenue is small; the money is in platform APIs, studio compliance and enterprise Article 50 audits. Standards politics are slow. Each platform may build its own badge, which is also the wedge: fragmentation is the customer's problem. Detection-based competitors (Pangram raised $9M) will keep losing the arms race, which is why this is a provenance product, not a classifier.

**Where to start.** The B2B side has budget today: game studios facing disclosure and consent obligations, EU enterprises facing the 2 December marking deadline, and insurers with a fraud budget. Build the individual-creator and student registry as the flywheel on top of that revenue.

## Why #2: certification and compliance for physical AI

Robotics raised $18.8B in 2026 through August, yet no 1X NEO had a verified customer delivery by July, humanoid autonomy runs at 60–70% with teleoperators filling the gap, and READY Robotics died because buyers preferred integrated hardware vendors to pure software. Meanwhile two hard EU dates land within five months: Cyber Resilience Act incident reporting on 11 September 2026 and the Machinery Regulation on 20 January 2027, which makes AI safety functions high-risk and forces third-party conformity assessment. The incumbents are TÜV, UL and Pilz, none AI-native. Independence is the product; a lab certifying itself is worthless. Start with the compliance software (cheap, dated demand), and build the accredited test floor once revenue supports it.

## Why #3 and #4: eldercare agent and scam-victim intervention

Both are "AI authorised to act on your behalf inside a regulated, liability-bearing, multi-party process", the one consumer pattern that survived every sweep. Seniors lost $7.75B to fraud in 2025, FBI IC3 losses hit a record $20.9B, and the state of the art for families is still "agree a code word". The care-labour shortage is demographic, not cyclical, and facility-side monitoring has raised over $100M while the family-admin side is served by thin apps. Neither product is chat-shaped: they require power-of-attorney verification, HIPAA agreements, outbound calls as a representative, bank integrations and a human escalation desk, which is exactly what ChatGPT and Gemini avoid. Both are distribution-heavy; the eldercare agent sells through adult children and insurers, the intervention platform through banks and exchanges.

## What changed from the first pass, and why

The first pass ranked a safety runtime for coding agents first because it fit a systems engineer. Removing that constraint exposed two things. First, agent security is the most crowded lane in AI: ten agentic-security startups raised $3.6B, seven of twelve July–August cyber rounds were agent protection, and Docker, NVIDIA, Cloudflare, Tencent, Anthropic and OpenAI all shipped sandboxes in 2026, with Daytona retreating to closed source under the pricing pressure. Second, the truly unowned territory is trust and verification for humans and institutions, which needs regulatory, industry and go-to-market skills more than kernel skills. The recovery-and-receipts slice of agent safety is still real pain, but it is a feature-sized wedge in a well-funded category, so it ranks eleventh.

## What to avoid, on the evidence

- **Anything chat-shaped for consumers.** ChatGPT (1B MAU) absorbed personal finance, health Q&A, shopping, travel and home repair in 2026; ~40% of VC-funded AI startups have shut.
- **Coding agents, AI IDEs and agent runtime security.** Lab surfaces plus a $3.6B funded field.
- **Generic MCP infra, agent memory, sandboxes, gateways, evals.** Open-sourced by NVIDIA, Cloudflare, Microsoft and the labs; Promptfoo and Stainless were bought.
- **Scribes, prior auth, big-law, customer support, freight voice.** Decided at $1–16B valuations and now bundled by Claude for Healthcare and ChatGPT Health.
- **World models, humanoid hardware, autonomous labs' model layer.** $1B-plus rounds and 97% of humanoid units are Chinese.
- **Player-facing AI in games, AI companions, AI therapy, AI pendants.** Review-bombed, sued, banned in four to seven states, or acquired and shut.
- **Businesses that assume agent-payment volume today.** x402 was at ~$28k per day in March 2026.

## Thirty-day validation for the top pick

1. **Week 1.** Pick the paying wedge. Interview five game-studio producers or publisher legal leads on disclosure and SAG-AFTRA consent tracking, and five EU marketing or compliance leads on the 2 December marking deadline. Ask what evidence they need and what they pay today.
2. **Week 2.** Ship a minimal capture-time provenance tool for one creator group (indie musicians or students) that signs drafts and process logs and produces a shareable proof page. Post it where the false-accusation and slop anger lives: r/mildlyinfuriating, r/WeAreTheMusicMakers, Hacker News.
3. **Week 3.** Approach two platforms already running badges (Spotify Verified, Nexus Mods tiers, the Authors Guild registry) with an interoperability proposal. Their answer tells you whether a cross-platform registry is welcome or resisted.
4. **Week 4.** Decide. Proceed if two B2B buyers will pilot and one platform will interoperate. If the platform answer is hostile, pivot the same technology to the insurance-claims vertical (#5), which has a fraud budget and no platform dependency.

## Research limitations

All ten sweeps shared one web-search budget of 200 calls, which ran out midway: the enterprise sweep got 12 searches, and the infra, sentiment and investor sweeps had none, so those three worked from GitHub-hosted digests and mirrors only. The sandbox proxy blocked direct fetches of Reddit, X, Hacker News, TechCrunch and nearly all press and VC sites, so sentiment comes through digests and search snippets. Funding figures and dates should be spot-checked before they go into a pitch. The verticals least researched are immigration and courts, government procurement, HR, hospitality, agriculture business and emerging-market SMB software; treat them as unknown, not empty.

## Addendum: deep dives reorder the top three

Three adversarial deep dives (see `deep-dives/00-comparison.md`) were run after this synthesis. The provenance registry does not survive as a venture: free incumbents, no regulatory pull for proving humanity, and a contested primitive. It remains viable only as an EU Article 50 and studio compliance SaaS. Physical-AI certification holds as a capital-efficient compliance business with the lab as a later option. The eldercare authority agent rises to first as the only candidate with a $1B comparable doing the same job, a priced willingness-to-pay ladder, and a tailwind that cannot slip.
