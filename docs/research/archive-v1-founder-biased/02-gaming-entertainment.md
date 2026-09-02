# Sector report 2 — AI + Gaming / Entertainment / Media

*Research agent output, 2026-09-02. Rests on 32 searches; primary pages (DeepMind, TechCrunch, GDC, PC Gamer) were egress-blocked, so figures come from search-result extracts and should be re-verified before external use.*

## 1. Where the big players are (and the gaps)

**Playable world models are now a capital-intensive lab race — not a startup lane.**
- Google Genie 3 (Aug 2025) generates navigable worlds at 24 fps/720p, consistent "for a few minutes"; opened as Project Genie to AI Ultra subscribers Jan 29 2026 ([Wikipedia](https://en.wikipedia.org/wiki/Genie_(world_model)), [Google](https://blog.google/innovation-and-ai/models-and-research/google-deepmind/project-genie/)).
- World Labs raised $1B (Feb 2026), shipped Marble and a public World API (Jan 2026) ([Silicon Republic](https://www.siliconrepublic.com/start-ups/fei-fei-li-world-labs-raises-1bn-to-spatial-intelligence-ai-world-models-marble), [World Labs](https://www.worldlabs.ai/blog/announcing-the-world-api)).
- Decart raised $300M at ~$4B (May 2026), but its flagship Oasis 3 is now pitched at autonomous-driving simulation, with sub-35ms latency infra as the product ([SiliconANGLE](https://siliconangle.com/2026/05/18/decart-raises-300m-ai-optimization-software-world-models/), [TNW](https://thenextweb.com/news/decart-300-million-radical-ventures-world-models)).
- Odyssey raised $310M at $1.45B (June 2026) ([TechCrunch](https://techcrunch.com/2026/06/17/world-model-maker-odyssey-nabs-1-45b-valuation-backed-by-amazon-and-other-big-names/)); Runway launched GWM-1 (Jan 2026) and text/image "Game Worlds" ([AIbase](https://www.aibase.com/news/19325)).
- General Intuition (Medal spinout) took a $133.7M seed and is raising $300M at ~$2B; its moat is Medal's 2B clips/year with controller-input labels — OpenAI reportedly tried to buy Medal for ~$500M ([TechCrunch](https://techcrunch.com/2026/06/18/general-intuition-in-talks-to-raise-300m-at-around-2b-valuation/), [TNW](https://thenextweb.com/news/general-intuition-300m-world-models-gaming-data)).

**Gap:** none of these output anything a Unity/Unreal/Godot project can ship: no collision, nav, LOD, determinism, multiplayer, or export. The "last mile" from generated world to engine is unowned.

**AI NPCs stalled.** Inworld repositioned from NPC studio to general real-time AI runtime; Convai is now the default plug-in, yet "as of mid-2026 there isn't a widely-played game popular because of its AI NPCs… experiment, not release" ([Loreweaver](https://loreweaver.ink/insights/inworld-convai-alternatives/), [Frisson Labs](https://www.frisson-labs.com/ai-npcs-2026)). Meanwhile, on-device inference matured: LLMUnity and Runtime Local LLM (llama.cpp, Vulkan/Metal, Quest/Android) exist as plugins ([DEV.co](https://dev.co/ai/frameworks/llmunity), [Georgy Dev](https://solutions.georgy.dev/runtime-local-llm)). SAG-AFTRA's 2025 agreement requires consent for digital replicas and pays at least 7.5x scale for "real-time generation" chatbots — a cost wall for cloud-LLM NPCs with union voices ([Davis+Gilbert](https://www.dglaw.com/sag-aftras-new-video-game-agreement/), [SAG-AFTRA](https://www.sagaftra.org/sag-aftra-members-approve-2025-video-game-agreement)).

**Creation platforms are crowded and lock-in-heavy.** Rosebud (Steam export added 2026, browser-locked), Ludo (API/MCP beta Mar 2026), SPARQ ($8.5M seed), Unity AI open beta May 4 2026 ($10/mo per 1,000 credits, "reception has not been kind"), plus Ziva/Summer Engine/Godot-AI MCP in Godot ([Ludo](https://ludo.ai/compare/best-ai-game-makers), [TopVoices](https://thetopvoices.com/story/sparq-raises-dollar85m-to-democratize-ai-powered-game-development), [Vindler](https://vindler.solutions/blog/unity-ai-open-beta), [Ziva](https://ziva.sh/blogs/best-ai-tools-for-godot-2026)). Roblox's Cube "4D generation" (Feb 2026) generates interactive objects in-platform ([Roblox](https://about.roblox.com/newsroom/2026/02/accelerating-creation-powered-roblox-cube-foundation-model)).

**Adjacent:** ElevenLabs $500M at $11B, Dubbing v2 in 90+ languages ([CNBC](https://www.cnbc.com/2026/02/04/nvidia-backed-ai-startup-elevenlabs-11-billion-valuation.html)); QA agents: ManaMind €1.2M pre-seed (17B vision model), category total ~$35.8M with no Series B ([EU-Startups](https://www.eu-startups.com/2026/04/ai-game-testing-startup-manamind-lands-e1-2-million-to-automate-quality-assurance/), [Naavik](https://naavik.co/ai-gaming/the-state-of-ai-for-game-qa/)); Character.AI settled the teen-suicide suits and banned under-18 open chat ([Fortune](https://fortune.com/2026/01/08/google-character-ai-settle-lawsuits-teenage-child-suicides-chatbots/)); Janitor AI claims 2.5M DAU, 70% female; the roleplay-chat market is ~$625M (2026) heading to $1.9B by 2031 ([Implicator](https://www.implicator.ai/janitor-ai-draws-2-5-million-daily-users-to-adult-roleplay-app/), [JustAINews](https://justainews.com/blog/spicychat-vs-janitor-ai-which-one-should-you-use/)).

## 2. Sentiment: gamers, developers, Steam

- **Gamers punish player-facing AI.** Arc Raiders, 1666 Amsterdam and Clair Obscur drew backlash after AI assets were found; Running With Scissors cancelled a game one day after reveal over AI-looking art; publishers now insert anti-AI contract clauses ([GamesRadar](https://www.gamesradar.com/games/the-backlash-against-gen-ai-in-video-games-proves-voting-with-your-wallet-works/)). Machine-translated text earns "Very Negative" localized reviews ([Alconost](https://alconost.com/en/blog/steam-language-mix-indies)).
- **Developers are souring:** 52% of GDC 2026 respondents say gen-AI harms the industry (30% prior year); 28% laid off in two years (33% US) ([GDC](https://gdconf.com/article/gdc-2026-state-of-the-game-industry-reveals-impact-of-layoffs-generative-ai-and-more/)). Epic/Unity/Activision cut ~8,400 jobs in Q1 2026 ([tech-insider](https://tech-insider.org/video-game-industry-layoffs-2026/)).
- **Steam's Jan 16 2026 rewrite** exempts behind-the-scenes tools (code assistants, QA) and targets content players consume; ~1 in 5 store listings now carry a disclosure, 40% of new releases in one June week ([PC Gamer](https://www.pcgamer.com/software/ai/steam-updates-ai-disclosure-form-to-specify-that-its-focused-on-ai-generated-content-that-is-consumed-by-players-not-efficiency-tools-used-behind-the-scenes/), [tech-insider](https://tech-insider.org/steam-ai-disclosure-2026/)).
- **What devs on r/gamedev, r/godot want:** a project file they own, real-engine export, no browser lock-in; distrust of anything they "cannot open in a normal editor" ([Summer Engine](https://www.summerengine.com/blog/best-ai-game-maker-reddit)).
- **Streaming:** AI VTuber Neuro-sama became Twitch's most-subscribed channel (160k+ subs, Jan 2026); open-source clones (AIRI) and companion tools (Questie) are appearing ([Cybernews](https://cybernews.com/ai-news/twitch-neuro-sama-reddit-vtuber/), [explainx](https://explainx.ai/blog/airi-ai-vtuber-neuro-sama-guide-2026)).

**Implication:** the safe, growing zones are *invisible-to-player* tooling (exempt from disclosure, no backlash), *engine-side runtime* problems, and *domains labs won't touch* (adult, Twitch-native).

## 3. Startup ideas

### A. Engine-native autonomous playtest/QA harness
- **Pitch:** A Rust agent runtime that injects input, captures frames/GPU timings, and drives vision+state agents through Unity/Unreal/Godot builds (and console devkits) to find crashes, soft-locks and perf regressions in CI.
- **Customer:** mid-size studios/publishers cutting QA (10–15% of budgets; "testing efficiency" was a top GDC 2026 pain) ([Naavik](https://naavik.co/ai-gaming/the-state-of-ai-for-game-qa/), [WeTest](https://www.wetest.net/blog/wetest-at-gdc-2026-1186.html)).
- **Why now:** layoffs + Steam exemption for dev tools; category underfunded (~$35.8M total, no Series B).
- **Competitors:** ManaMind, modl.ai, Nunu.ai, Razer QA Companion ([Razer](https://www.razer.ai/qac/)). Moderately crowded; nobody owns the engine/devkit integration layer.
- **Lab-proof:** labs ship generic computer-use; they will not do deterministic replay, engine hooks, or console certification.
- **Moat:** engine integration depth, replay/crash corpus per studio, CI distribution.
- **Fit:** excellent (frame capture, input injection, headless rendering are systems/GPU work). Capital: medium (GPU compute for agents).
- **Scores:** market 7 · timing 8 · defensibility 8 · fit 9 · capital 6.

### B. "Last-mile" world-model-to-engine pipeline
- **Pitch:** Rust/GPU tool that ingests Marble/World API/Genie/3DGS output and emits game-ready levels: mesh extraction, collision, navmesh, LOD, occlusion, streaming, with Unity/Unreal/Godot exporters.
- **Customer:** indie/mid studios and archviz/VFX teams wanting generated environments they can actually ship.
- **Why now:** World API public (Jan 2026), 212 splat tools, engine plugins hitting 60 fps under 1M Gaussians ([RadianceFields](https://radiancefields.com/gaussian-splatting-statistics)), $1.5B poured into splat-adjacent companies.
- **Competitors:** many splat viewers; Polyvia-style plugins; World Labs may ship exporters. Medium crowdedness, thin on gameplay-readiness.
- **Lab-proof:** labs monetize generation, not engine runtime; every new model *increases* demand for conversion.
- **Moat:** format/engine expertise, perf, community of shipped levels. Risk: World Labs/Unity bundle exporters.
- **Fit:** very high. Capital: low.
- **Scores:** market 6 · timing 8 · defensibility 6 · fit 10 · capital 9.

### C. Local-first NPC runtime with union-safe voice
- **Pitch:** On-device dialogue/behavior runtime (llama.cpp-class inference in Rust, Vulkan/Metal, quantized, memory-budgeted for consoles/Quest) plus consent-tracked voice pipeline aligned to the 2025 SAG-AFTRA terms.
- **Customer:** studios that want AI characters but can't pay cloud inference at scale or 7.5x-scale chatbot rates.
- **Why now:** Inworld left the niche; cloud latency/cost is why AI NPCs haven't shipped ([Frisson Labs](https://www.frisson-labs.com/ai-npcs-2026)); OSS building blocks exist ([Llama-Unreal](https://github.com/getnamo/Llama-Unreal)).
- **Competitors:** Convai, LLMUnity (OSS), Nvidia ACE. Medium.
- **Lab-proof:** labs sell cloud tokens; shipping models inside game binaries on consoles is antithetical to their model.
- **Moat:** engine/console integration, deterministic behavior tooling, legal/consent workflow.
- **Fit:** high. Capital: low. Risk: demand still unproven (no hit AI-NPC game).
- **Scores:** market 6 · timing 6 · defensibility 8 · fit 9 · capital 8.

### D. Real-time AI VTuber / co-host engine (open-core)
- **Pitch:** Low-latency local pipeline (chat ingest → LLM → TTS → Live2D/VRM lip-sync → game-input agent) packaged for Twitch/YouTube creators, with a marketplace of personalities/rigs.
- **Customer:** VTubers and streamers wanting a Neuro-style co-host or solo companion.
- **Why now:** Neuro-sama tops Twitch subs; AIRI and Questie prove demand ([Cybernews](https://cybernews.com/ai-news/twitch-neuro-sama-reddit-vtuber/), [Questie](https://www.questie.ai/neuro-sama)); VTube Studio (92% positive, 3,100 reviews) shows creators pay for desktop tooling ([TheAISurf](https://theaisurf.com/ai-vtuber-tools/)).
- **Competitors:** AIRI (OSS), Questie, DIY. Low-medium.
- **Lab-proof:** Twitch-specific latency, rig/avatar formats, moderation and chat dynamics are outside lab scope; models are swappable.
- **Moat:** creator community, rig/personality marketplace, latency engineering.
- **Fit:** high (real-time audio/graphics). Capital: very low. Risk: VTuber-community anti-AI sentiment; platform policy.
- **Scores:** market 6 · timing 7 · defensibility 7 · fit 8 · capital 10.

### E. Roleplay-optimized inference infra for adult-adjacent platforms
- **Pitch:** Rust serving stack (long-context KV-cache reuse, lorebook/memory, character consistency, cheap open-weight models) sold to Janitor/SpicyChat-class platforms and self-hosters.
- **Customer:** roleplay platforms serving 130M monthly visits ([DualMedia](https://www.dualmedia.fr/en/janitor-has-a-game-changing-chatbot/)) and needing labs-free models.
- **Why now:** Character.AI's minors ban and settlements push adults to uncensored platforms; labs refuse the content.
- **Competitors:** generic vLLM/OpenRouter; few domain-specific. Low crowdedness on infra, high on apps.
- **Lab-proof:** by definition — labs won't serve NSFW.
- **Moat:** domain tuning, customer lock-in, reputational barrier to entry.
- **Fit:** strong technically; weaker on go-to-market/legal. Capital: medium (GPUs).
- **Scores:** market 7 · timing 8 · defensibility 9 · fit 7 · capital 6.

### F. Rules-enforcing solo/async tabletop DM
- Friends and Fables ($19.95–39.95/mo), AI Realm, CharGen already exist; demand comes from DM shortage ([Dungeons Deep](https://dungeonsdeep.ai/blog/friends-and-fables-review-2026), [Jenova](https://www.jenova.ai/en/resources/ai-dungeon-master)). Moat would be a real 5e rules engine + VTT + licensed IP (Hidden Door's model, [Forbes](https://www.forbes.com/sites/charliefink/2025/08/14/hidden-door-turns-fan-worlds-into-licensed-revenue-sharing-story-platforms/)), but a ChatGPT "DM mode" erodes the low end and founder fit is weakest.
- **Scores:** market 6 · timing 6 · defensibility 5 · fit 5 · capital 8.

## 4. Ranked shortlist

| Rank | Idea | Total /50 |
|---|---|---|
| 1 | A. Engine-native playtest/QA harness | 38 |
| 2 | B. World-model-to-engine last-mile pipeline | 39* |
| 3 | D. AI VTuber/co-host engine | 38 |
| 4 | E. Roleplay inference infra | 37 |
| 5 | C. Local-first NPC runtime | 37 |
| 6 | F. Tabletop DM | 30 |

*B scores highest on raw points but A ranks first because it has budget-line demand today and no disclosure risk; B's biggest risk is World Labs/Unity bundling exporters. D is the best zero-capital bet.

**Verdict:** The lab-funded world-model race ($1B+ rounds for World Labs, Odyssey, Decart, General Intuition) has made "generate the game" a non-starter for a solo founder, while player-facing AI faces real backlash and a Steam label on 20% of listings. The durable openings for a Rust/GPU engineer are behind the curtain — engine-side runtimes, QA automation, real-time creator tooling, and infra for content labs refuse — where every model upgrade lowers your costs rather than killing you. Pick the wedge with a paying customer this year (studio QA or streamers), keep the model pluggable, and let engine integration be the moat.
