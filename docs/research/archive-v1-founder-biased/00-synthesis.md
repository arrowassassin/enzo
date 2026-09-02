# AI startup opportunity research — synthesis and ranking

*Date: 2026-09-02. Method: eight parallel research sweeps (six sectors, one social-sentiment sweep across Reddit/X/HN/Product Hunt, one investor-thesis and macro sweep), each run with live web search. Sector reports are in this folder (01–08) with sources.*

## The brief

Find an AI-domain project with real startup potential for a solo (or tiny-team) Rust/systems/GPU engineer with little capital, where the product is **not** something Anthropic, OpenAI or Google can erase with a model update or a feature release. Any sector was allowed: security, entertainment, gaming, utility, infra, physical world.

## The one-paragraph answer

The clearest opening in September 2026 is a **vendor-neutral safety and recovery runtime for AI agents that run on machines the labs don't control**: kernel-level enforcement of what an agent may touch, snapshot-and-undo for everything it changes, default-deny network egress with credential brokering, and a tamper-evident "flight recorder" that insurers and EU auditors can accept as evidence. Five of the eight independent sweeps converged on this gap from different directions (security, consumer, sentiment, investor, infra). Demand and pain were both proven in 2026 by OpenClaw (388k GitHub stars, 1,184 malicious skills, ~200k exposed instances), the Cursor DuneSlide sandbox escape, and the year's loudest Hacker News thread ("an AI agent deleted our production database"). The labs ship sandboxes and permissions **for their own agent only**, and Anthropic's own sandbox docs list the bypasses; nobody owns recovery, neutrality, or the evidence layer. The runner-up, a **Rust real-time runtime for deploying robot policies and vision models on edge silicon**, is the most lab-immune territory found anywhere, but its customers are fewer and poorer today.

## How the candidates were scored

Every sweep produced 4–6 ideas with its own scores. The ten strongest cross-sector candidates were re-scored on one rubric, weighted to match the brief:

| Criterion | Weight | Meaning |
|---|---|---|
| Lab-immunity | ×2 | Can a frontier lab kill it with a release? 10 = structurally impossible |
| Founder fit | ×1.5 | Rust / systems / GPU / real-time, solo, little capital |
| Evidence | ×1.5 | Strength and independence of demand signals found (funding, incidents, sentiment, RFS) |
| Market | ×1 | Size of the paying market in 2026–28 |
| Timing | ×1 | Is there a catalyst now? |
| Capital | ×1 | 10 = can be started with near-zero money |
| Distribution | ×1 | Can a technical solo founder reach the first ten paying customers? |

Maximum score is 90.

## Ranked contenders

| # | Idea | Lab-imm. | Fit | Evid. | Mkt | Time | Cap | Dist | **Score** | Sweeps that surfaced it |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **Agent blast-radius runtime** — kernel enforcement + undo + egress/credential broker + flight recorder, for any agent (OpenClaw, Claude Code, Codex, Cursor, goose) | 7 | 10 | 10 | 8 | 9 | 9 | 7 | **77** | Security, Consumer, Sentiment, Investor, Infra |
| 2 | **Physical-AI edge runtime** — Rust real-time runtime for VLA/imitation policies on Jetson/Hailo/Rockchip, then cross-NPU deployment and teleop-data QA | 10 | 10 | 7 | 6 | 8 | 9 | 5 | **73.5** | Physical AI, Investor (YC RFS "Data for the Real World", "OS for the Physical World") |
| 3 | **Sovereign open-weight inference stack** — single signed Rust binary for on-prem/air-gapped, differentiated by AMD/Apple/Tenstorrent kernels | 9 | 10 | 7 | 7 | 8 | 7 | 5 | **70** | Infra (two ideas merged), Vertical (EU legal on-prem), Investor |
| 4 | **Family voice-scam shield** — on-device call screening and family-voice verification for seniors, paid by adult children or credit unions | 9 | 6 | 8 | 8 | 9 | 6 | 4 | **66** | Security, Consumer, Sentiment, Investor (YC "AI for Aging") |
| 5 | **Engine-native game QA harness** — Rust agent runtime that drives Unity/Unreal/Godot builds in CI to find crashes and perf regressions; plus world-model-to-engine "last mile" export | 8 | 9 | 6 | 6 | 8 | 7 | 6 | **65.5** | Gaming |
| 6 | **Provenance / "proving you're human"** — free C2PA CA + signing sidecar for EU Art. 50 (live Aug 2 2026, marking Dec 2 2026) + human-authorship attestations | 8 | 7 | 7 | 6 | 9 | 8 | 5 | **65** | Security, Sentiment, Investor (YC "Proving You're Human", Pangram $9M) |
| 7 | **Open local life-log** — the Limitless/Rewind that can't be acquired out from under you (Rust daemon + open pendant, all local, exposed via MCP) | 7 | 9 | 7 | 6 | 8 | 5 | 6 | **63** | Consumer |
| 8 | **Post-acute healthcare interop engine** — fax/HL7/X12/EVV ingestion into a system of record for home health and DME | 8 | 7 | 6 | 7 | 8 | 6 | 3 | **59.5** | Vertical |
| 9 | **Model-churn regression suite** — detects when forced upgrades/deprecations change your outputs | 4 | 7 | 6 | 6 | 7 | 9 | 6 | **55.5** | Sentiment |

## Why #1: the agent blast-radius runtime

**The gap.** In 2026 the agent runs on the user's laptop, NAS, CI box or on-prem server, not in the lab's cloud. The labs' answers (Anthropic sandbox-runtime, OpenAI Agents SDK sandboxes, Managed Agents) cover only their own product and the cloud path. Practitioners at Infosec Europe 2026 concluded prompt injection is unsolved and the only defense is constraining what the agent *can do*. Three things are unowned:

1. **Enforcement below the app.** eBPF/LSM on Linux, Seatbelt on macOS, per-agent and per-tool-call policy, regardless of which agent or model. Cursor's CVSS 9.8 sandbox escape shows app-level sandboxes fail.
2. **Recovery.** Filesystem snapshots, transactional writes, replayable network log, one-command undo. The Hacker News thread with 638 points was about an unrecoverable deletion; Claude Code's own tracker has 168 destructive-action issues. Labs ship permissions, not undo.
3. **Evidence.** A cryptographically chained record of every tool call, approval and outcome. Insurers moved to explicit AI-liability policies on Jan 1 2026 and price on "containment and monitoring controls"; an insurer will not accept a self-attested trace from the vendor whose model caused the loss. EU AI Act Article 12 logging obligations follow.

**Why the labs won't kill it.** Neutrality is the product. Anthropic will never ship kernel enforcement for OpenClaw or Codex; OpenAI will never certify Claude Code's actions for an insurer. Every model improvement makes agents *more* autonomous and increases demand for a seatbelt. The buyer is a security team, MSP, self-hoster or insurer, not the developer choosing an IDE.

**Honest risks.** This is the candidate closest to lab territory and to the workspace project just scrapped, so discipline matters: do not build an agent, an IDE, or a chat surface. Agent security drew $3.6B in 2026 and there are funded players at the cloud layer (Sysdig, ARMO, AIR, Docker Sandboxes) and open-source projects at the endpoint (nono from the Sigstore team, microsandbox, clawk, omnigent). None combine kernel enforcement, undo and an insurer-grade record; that combination and the insurer/audit channel are the moat. Plan for the acquirer outcome (Koi sold to Palo Alto for ~$400M within a year) as an acceptable path.

**Shape of the business.** Open-core. Free single-binary runtime for individuals and the OpenClaw crowd; paid policy packs, fleet management and the signed evidence service for teams; insurer and MSP partnerships as the channel.

## Why #2: the physical-AI edge runtime

**The gap.** Open VLA weights (openpi, SmolVLA, GR00T-N) are everywhere and edge hardware is $130–$249, but deployment is Python-only, LeRobot's own inference produces wrong results on Jetson ARM64, VLA on Orin runs at 6–9 Hz, and none of the Rust ML runtimes (candle, burn, tract, mistral.rs) target an NPU. Rockchip's toolkit has 459 open issues; Hailo's runtime breaks on `apt upgrade`. A "vla.cpp" paper in June 2026 shows the gap is recognised but unfilled with real-time guarantees.

**Why the labs won't kill it.** A frontier-model update is irrelevant to hard real-time control loops, silicon fragmentation, calibration and URDF plumbing. Labs ship weights, not board-support-level runtimes for other people's chips. Even NVIDIA (the real threat) is NVIDIA-only; cross-vendor is the defense.

**Honest risks.** Robotics software receives ~1.6% of robotics capital; customers are startups in a shakeout. First revenue is slower and smaller; the natural second acts (cross-NPU industrial vision runtime, teleop-dataset QA for the $67K-per-50K-episode data vendors) are where the money is. This is the right pick if the founder can bootstrap for 12+ months and wants the least-contested ground.

## Why not the others (briefly)

- **Sovereign inference stack (#3)** is a strong business with the highest founder fit, but the engine layer is crowded (vLLM via Red Hat, SGLang, mistral.rs) and it is an enterprise sale. It combines naturally with #1 into a "private AI appliance" later; do not start there.
- **Voice-scam shield (#4)** is the largest human need found ($7.75B senior losses in 2025, AI fraud +1,210%) and the labs will not listen to third-party phone calls. The blocker is distribution and wiretap consent; it needs a consumer or channel co-founder.
- **Game QA harness (#5)** has budget-line demand and is exempt from Steam's AI disclosure, but studio sales during 8,400 Q1 layoffs are slow and the QA category has raised only ~$36M in total.
- **Provenance (#6)** has a hard regulatory catalyst this quarter but no proven revenue model; a free CA is a public good before it is a company.
- **Life-log (#7)** needs hardware capital; **post-acute interop (#8)** needs a clinical co-founder; **model-churn suite (#9)** is exactly the kind of thing OpenAI already bought (Promptfoo).

## What to avoid (the evidence)

The "Killed by AI" graveyard (111 entries, 71 in 2025–26) and the funding data agree on what dies:

- Anything whose value is access to, or light shaping of, a frontier model's output (Jasper, Writer's early product, 200+ wrappers cannibalised by ChatGPT features).
- Horizontal agent layers, even lab-owned ones (Relay.app, OpenAI's own Operator, Atlas, GPT Store).
- Coding assistants and AI IDEs (Claude Code Security alone knocked CrowdStrike, JFrog and GitLab down 8–25% in a day).
- Standalone LLM red-teaming, evals and guardrails for the lab's own agent (Promptfoo, Lakera, Protect AI, SPLX, CalypsoAI all acquired within a year).
- Consumer AI hardware without an open-firmware promise (Humane, Rabbit, Limitless).
- AI companions and AI therapy (wrongful-death settlements, 70+ state bills, seven states restricting AI therapy).
- Regulated health AI without a clinical partner (Woebot, Kintsugi, Cydoc).

## 30-day validation plan for the top pick

1. **Week 1.** Ship a Linux-only Rust daemon that wraps any agent process, enforces a deny-by-default file/network policy via eBPF/LSM, and snapshots the working tree before each tool call. Target OpenClaw and Claude Code first.
2. **Week 2.** Post to r/LocalLLaMA, r/selfhosted and Hacker News as "undo for AI agents". Measure stars, issues and inbound from security teams.
3. **Week 3.** Interview five people: two security leads whose developers run coding agents, one MSP, one AI-liability underwriter (Armilla, Corgi, HSB), one EU AI Act consultant. Ask what evidence they would need to accept.
4. **Week 4.** Decide. Proceed if there are 500+ stars, three teams asking for fleet policy, or one underwriter saying the record would change pricing. Otherwise pivot to the physical-AI runtime, whose first step is a Jetson Orin + SO-101 demo running a LeRobot checkpoint at a deterministic control rate.

## Research limitations

Every sweep ran inside a sandbox whose proxy blocked direct fetches of Reddit, X, Hacker News, TechCrunch and most press sites. Sentiment therefore comes through search-index snippets, secondary coverage and GitHub (which was reachable) as a proxy for builder demand. Funding figures and dates are from indexed extracts of the linked sources and should be spot-checked before going into a deck or a pitch.
