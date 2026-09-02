# Sweep 3 — AI × Gaming / Interactive Entertainment

*Unbiased rerun, 2026-09-02. No founder-profile constraint. 40 searches; nearly all news domains egress-blocked, so evidence relies on search snippets plus reachable GitHub pages.*

## 1. State of play (2025–26)

**Capital is flooding into world models and 3D gen, not games.** General Intuition (Medal spin-out) raised a $320M Series A at $2.3B and is reportedly seeking $6B for a *robotics* push ([TechCrunch](https://techcrunch.com/2026/06/25/general-intuitions-2-3b-bet-that-video-games-can-train-ai-agents-for-the-real-world/), [PYMNTS](https://www.pymnts.com/news/investment-tracker/2026/general-intuition-aims-to-raise-capital-at-6-billion-valuation-to-power-ai-robotics/)). World Labs raised $1B (~$5B valuation) ([PYMNTS](https://www.pymnts.com/artificial-intelligence-2/2026/world-labs-raises-1-billion-to-scale-spatial-ai/)). Meshy closed $400M at $1.5B on ~$40M ARR ([Naavik](https://naavik.co/ai-gaming/inside-meshys-400m-raise-for-3d-assets/)). Decart pivoted Oasis 3 to autonomous-driving sims at $0.02/sec API ([TechCrunch](https://techcrunch.com/2026/06/10/decarts-new-world-model-can-simulate-hours-of-photorealistic-driving-with-some-caveats/)). Odyssey-2 Pro shipped a dev API ([Odyssey](https://odyssey.ml/introducing-odyssey-2)). Google's Project Genie went to AI Ultra subscribers in Jan 2026 ([9to5Google](https://9to5google.com/2026/01/29/google-project-genie/)); Genie 3 still holds consistency only "a few minutes" ([DeepMind](https://deepmind.google/blog/genie-3-a-new-frontier-for-world-models/)). Key inference: the frontier world-model players are all heading toward robotics/enterprise APIs, leaving "make it an actual game" unowned.

**AI-native games have not produced a Western hit.** Latitude (AI Dungeon) has 1.5M MAU but only ~$1.4–1.8M ARR and pivoted to a UGC platform in April 2026 ([IntelPilot](https://www.intelpilot.ai/company/latitude/6a028ebda6715bdc30963e7e)). Hidden Door raised only $7M total ([Tracxn](https://tracxn.com/d/companies/hidden-door/__DEEpimT5lup2Ka4OsbHxTGewc-6mqkwjQXwWx_E49y4)). The one scale datapoint is China: Tencent's Game for Peace AI-NPC features hit 110M cumulative users and 17.7M peak DAU ([BigGo](https://finance.biggo.com/news/1c5f0b6a-2859-4290-aa3b-f8ab2afb8335)) — AI as a *teammate inside an existing hit*, not a new genre. Token costs at hit scale can "wipe out revenue" ([Medium](https://ayushhh.medium.com/the-bearish-case-against-ai-npc-startups-37c2c43da466)).

**Backlash is the binding constraint.** 30.8% of 2026 Steam releases carry an AI disclosure (10.9% in 2024, 19.9% in 2025) but AI-disclosed games take only 10–27% of estimated sales ([Cinevva](https://app.cinevva.com/news/2026-07-20-steam-ai-disclosure-study), [PC Gamer](https://www.pcgamer.com/gaming-industry/steam-week-in-review-take-cover-because-it-looks-like-more-than-half-of-steam-games-will-have-an-ai-disclosure-by-2027-2028/)). 52% of GDC 2026 respondents say gen-AI hurts the industry ([Naavik](https://naavik.co/podcast/gdc-2026-ai-ugc-and-the-new-reality-of-game-funding/)). Clair Obscur was disqualified from the Indie Game Awards; Postal: Bullet Paradise was cancelled one day after reveal; publishing contracts now carry anti-AI clauses ([GamesRadar](https://www.gamesradar.com/games/the-backlash-against-gen-ai-in-video-games-proves-voting-with-your-wallet-works/)). Developers called Project Genie a "plagiarism engine"; Roblox, Take-Two, Unity shares fell on its launch ([Digital Watch](https://dig.watch/updates/game-developers-fear-job-loss-as-google-unveils-genie-3)). Nexus Mods now mandates three-tier AI tagging ([Shacknews](https://www.shacknews.com/article/150216/nexus-mods-gen-ai-tagging-moderation)). Epic's UEFN "Conversations" is still experimental and unpublishable after the SAG-AFTRA Vader ULP ([Engadget](https://www.engadget.com/gaming/fortnite-is-about-to-unleash-ai-powered-npcs-172728548.html)).

**Behind-the-scenes B2B is where revenue exists — and where it gets crowded fast.** Sett (AI UA agents) raised $30M on "tens of millions" in revenue ([Ctech](https://www.calcalistech.com/ctechnews/article/rjxbyqisze)), while Reforged Labs (same niche) shut down ([GameBastion](https://gamebastion.com/n/ai-startup-reforged-labs-shuts-down-as-the/517960)). Game-QA startups have raised a combined $35.8M with none at Series B ([Naavik](https://naavik.co/ai-gaming/the-state-of-ai-for-game-qa/)). Prediction markets: Polymarket runs 232 esports markets, ~$9.3M daily esports volume; Genius Sports signed both Polymarket and Kalshi for traditional sports data ([PillarLab](https://pillarlabai.com/blog/esports-prediction-markets-2026/), [iGB](https://igamingbusiness.com/prediction-markets/genius-sports-enters-prediction-markets-polymarket-kalshi-partnerships/)). Preservation: GoldenEye took 9 years to decompile; N64Recomp has 8.1k GitHub stars; LLM-assisted decomp is emerging ([Time Extension](https://www.timeextension.com/news/2026/08/after-9-years-of-work-goldeneye-007-n64-is-now-100percent-decompiled), [N64Recomp](https://github.com/N64Recomp/N64Recomp), [HF blog](https://huggingface.co/blog/MatthewReingold/n64-decomp-dev-blog)).

## 2. Startup ideas

Scores: Market / Timing / Lab-immunity / Whitespace / Moat / Capital-efficiency / Demand-evidence.

### A. Provenance & AI-disclosure compliance layer for studios ("SOC2 for gen-AI in games")
**Pitch:** Pipeline-integrated audit trail (Perforce/DCC/engine plugins) that tags every asset's AI exposure, enforces publisher anti-AI clauses and SAG-AFTRA consent rules, and auto-generates Steam/Nexus/awards disclosures with evidence.
**Why now:** binary Steam flag punishing everyone equally; awards disqualifications; contract clauses; SAG-AFTRA's 2025 IMA replica-consent regime ([SAG-AFTRA](https://www.sagaftra.org/sag-aftra-members-approve-2025-video-game-agreement)); Nexus tiered tags. **Competitors:** none game-specific found. **Lab-immunity:** labs are the audited party, not the auditor. **Moat:** integrations + publisher policy library + legal-grade evidence chain.
**Scores:** 6 / 9 / 9 / 8 / 7 / 8 / 7.

### B. AI-assisted decompilation & native-port factory for back catalogs
**Pitch:** LLM + N64Recomp-style toolchain that turns publisher ROMs (lost-source titles) into native, moddable PC/console ports in weeks, sold as remaster-as-a-service or rev-share.
**Why now:** recompilation is "the most productive corner of preservation"; LLM decomp works; GoldenEye shows 9-year manual timelines. Players *like* this AI use. **Competitors:** hobbyist projects; no commercial vendor surfaced. **Lab-immunity:** legally grey, per-platform toolchains, licensing relationships.
**Scores:** 4 / 7 / 9 / 8 / 6 / 8 / 6.

### C. Esports data + integrity oracle for prediction markets
**Pitch:** Official-grade live data, settlement feeds and AI match-fixing/smurf detection for Tier-2/3 esports, sold to Kalshi/Polymarket-class venues and tournament organizers.
**Why now:** CFTC-regulated venues now list LoL/CS2/Valorant/Dota majors; Genius Sports locked up traditional sports but not esports. **Competitors:** GRID, PandaScore, Bayes Esports, Sportradar — established but not prediction-market-focused.
**Scores:** 6 / 8 / 9 / 5 / 8 / 5 / 7.

### D. Drop-in AI teammate/companion for existing multiplayer hits
**Pitch:** Per-game behavioral agent + voice that fills empty squad slots, calls plays, and coaches — licensed to live-service studios, opt-in for players.
**Why now:** the only mass-scale proof of AI in games is Tencent's 17.7M peak-DAU teammate feature; AI coaching demand is real. **Competitors:** Ubisoft Teammates (in-house), Inworld/Convai, General Intuition. **Moat:** per-title behavior data; token-cost engineering.
**Scores:** 8 / 7 / 6 / 6 / 6 / 4 / 8.

### E. Consent-first voice-replica licensing exchange for games and mods
**Scores:** 5 / 8 / 7 / 5 / 6 / 7 / 6. ElevenLabs is the main threat.

### F. Game runtime over rented world models ("Unity for Genie/Odyssey")
**Scores:** 7 / 7 / 4 / 8 / 4 / 3 / 4. Most exciting, least defensible.

**Considered and rejected as crowded or lab-exposed:** AI QA agents, AI dungeon masters, UA creative agents, player-support bots, prompt-to-game platforms, generic AI-NPC middleware.

## 3. Ranked shortlist
1. **A – Provenance/disclosure compliance** (avg 7.7).
2. **C – Esports integrity/data oracle** (6.9).
3. **B – Decomp/port factory** (6.9).
4. **D – AI teammate** (6.4).
5. **E – Voice licensing exchange** (6.3).
6. **F – World-model runtime** (5.3).

**Verdict:** The money in AI+gaming is pouring into world models that are quietly leaving games for robotics, while every player-facing AI product is getting review-bombed and no Western AI-native game has cleared ~$2M ARR. The durable startups here are behind-the-scenes plumbing that treats the labs' models as commodities and the backlash as a feature — compliance, rights, integrity, preservation. Anything that shows players the word "AI" needs Tencent-scale distribution to survive.
