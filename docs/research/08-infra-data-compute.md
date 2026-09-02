# Sweep 8 — AI Infrastructure, Data, Compute and Developer Layers

*Unbiased rerun, 2026-09-02. No founder-profile constraint. **WebSearch was unavailable** (session budget exhausted before this sweep began) and every news/blog/social domain was egress-blocked, so all evidence is GitHub-derived (stars, issues, READMEs as of 2026-09-02). Funding rounds and valuations for named companies could not be verified and are deliberately omitted. Practitioner sentiment is proxied by GitHub issues/discussions.*

## Landscape signals that shape the ideas
- **Harness wars are over-supplied; infra under them is not.** OpenClaw 388k stars ([repo](https://github.com/openclaw/openclaw)), Hermes Agent (Nous) 240k ([repo](https://github.com/NousResearch/hermes-agent)), DeepSeek Harness 208k in three weeks ([repo](https://github.com/deepseek-ai/deepseek-harness)), Codex 121k, Claude Code 144k. Nous effectively pivoted from "open-weight lab" to agent harness + subscription.
- **Sandboxes are being absorbed.** Daytona (71.8k stars) took its core closed in June 2026 ([README notice](https://github.com/daytonaio/daytona/blob/main/README.md)); NVIDIA shipped OpenShell (8.5k) and NemoClaw (22k) ([NemoClaw](https://github.com/NVIDIA/NemoClaw)); Anthropic ships sandbox-runtime ([repo](https://github.com/anthropics/sandbox-runtime)); Cloudflare shipped "computer" (8.9k) ([repo](https://github.com/cloudflare/computer)); Tencent CubeSandbox 11.6k, OpenSandbox 14.9k, kubernetes-sigs/agent-sandbox 3.7k. Self-hosted microVM forkers (forkd 2.8k, zeroboot 2.4k) position against E2B/Modal ([forkd](https://github.com/deeplethe/forkd)).
- **Gateways/routers are commoditized.** LiteLLM 58k (now Rust core), Portkey 12.9k, NVIDIA Switchyard 2.7k ([repo](https://github.com/NVIDIA-NeMo/Switchyard)), plus a wave of "free-tier" routers (OmniRoute 60k, 9router 27k).
- **Token cost is the loudest practitioner pain.** rtk 78k ([repo](https://github.com/rtk-ai/rtk)), caveman 102k, headroom 68k, context-mode 20k; Claude Code issues asking for cost limits on unattended runs ([#66048](https://github.com/anthropics/claude-code/issues/66048), [#41859](https://github.com/anthropics/claude-code/issues/41859)).
- **Non-NVIDIA serving is fragmented and painful.** vLLM has separate community plugins for Ascend (2.7k), Metal (1.7k), Kunlun, Gaudi (52 stars), Neuron (49), Spyre, RBLN, MetaX, FlagOS; ROCm issues include perf regressions and "too many opt-in ROCm specific flags" ([#30064](https://github.com/vllm-project/vllm/issues/30064), [#21138](https://github.com/vllm-project/vllm/issues/21138)); TokenSpeed targets MI355X/MI455X and B300 ([repo](https://github.com/lightseekorg/tokenspeed)); Tenstorrent pays $1,500 bounties for kernel work ([issue](https://github.com/tenstorrent/tt-metal/issues/37828)).
- **RL environments became an industry.** Prime Intellect verifiers 4.6k + community-environments (265 forks, 224 PRs) ([verifiers](https://github.com/PrimeIntellect-ai/verifiers)); HF OpenEnv's technical committee lists Meta-PyTorch, Unsloth, Modal, Prime Intellect, NVIDIA, Mercor, Fleet AI, Microsoft, with Scale AI as supporter ([OpenEnv](https://github.com/huggingface/OpenEnv)); OpenClaw-RL (5.7k) backed by Fireworks and Tinker ([repo](https://github.com/Gen-Verse/OpenClaw-RL)); Tinker cookbook 4.1k; ART 10.7k.
- **Agent identity/authorization is unresolved at the protocol level.** MCP's top discussions: multi-user authorization, non-interactive OAuth, gateway-based authz, on-behalf-of token exchange ([discussions](https://github.com/modelcontextprotocol/modelcontextprotocol/discussions?discussions_q=sort%3Atop), [#214](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/214)); Casdoor rebranded to "Agent-first IAM" (14.3k); Microsoft agent-governance-toolkit 6.2k ([repo](https://github.com/microsoft/agent-governance-toolkit)); Google SAM 814. OpenClaw has 99 security-tagged issues incl. a bundled hook that "can silently hijack the agent" ([#8776](https://github.com/openclaw/openclaw/issues/8776)).
- **Payments rails exist but are thin.** x402 moved to a foundation (6.6k) ([repo](https://github.com/x402-foundation/x402)); AP2 3.2k ([repo](https://github.com/google-agentic-commerce/AP2)); Stripe MPP SDKs have 5–15 stars; ClawRouter 6.6k ([repo](https://github.com/BlockRunAI/ClawRouter)); Internet Court consortium 5.3k ([repo](https://github.com/internet-court/internet-court-skill)).
- **Weak OSS signal** (does not mean no market): AI-for-EDA, COBOL/mainframe agents, energy/datacenter constraints, expert-data marketplaces (Mercor has no public GitHub).

## Ideas

### 1. Vertical RL-environment studio with expert graders
**Pitch:** Build and license training-ready environments + human-expert reward models for one regulated vertical (finance ops, clinical coding, tax, RTL/EDA) sold to labs and to enterprises fine-tuning open weights.
**Why now:** OpenEnv/verifiers standardized the interface; Mercor, Fleet, Scale sit on the committee but the long tail of verticals is empty; Repo2RLEnv ([repo](https://github.com/huggingface/Repo2RLEnv)) shows appetite for auto-generated envs; synthetic task gen got 9.9k stars ([arc-task-gen](https://github.com/pathwaycom/arc-task-gen)).
**Competitors:** Prime Intellect Hub, Mercor, Surge, Scale, Handshake (crowded horizontally; open vertically).
**Lab immunity:** Labs are the buyers; they need humans and domain rights they do not have.
**Moat:** Proprietary grader corpora + credentialed expert network + env versioning tied to eval leaderboards.
**Scores:** Market 8 · Timing 9 · Lab-immunity 8 · Whitespace 6 · Moat 7 · Capital eff. 6 · Demand 8.

### 2. Neutral multi-silicon serving certification layer
**Pitch:** "Runs-on-anything" inference distribution: one tested build of vLLM/SGLang/TokenSpeed with kernels, quant recipes, and SLAs certified per chip (MI355X/MI455X, Tenstorrent Blackhole, Ascend, Gaudi, Trainium, Apple/Strix Halo), sold to neoclouds and sovereign DCs.
**Why now:** Plugin fragmentation (9+ vLLM hardware plugins), ROCm regressions, vendors paying bounties for software.
**Lab immunity:** Labs will never sell serving software; NVIDIA structurally cannot be neutral. Risk: AMD acqui-hires the team.
**Scores:** Market 8 · Timing 8 · Lab-immunity 9 · Whitespace 7 · Moat 6 · Capital eff. 4 · Demand 7.

### 3. Agent identity and delegated-authorization broker
**Pitch:** An IdP-agnostic "agent passport": issues scoped, revocable on-behalf-of credentials to agents, brokers MCP OAuth/token exchange, logs mandates, and plugs into x402/AP2/MPP for payments.
**Why now:** MCP auth threads are the most-voted unresolved topics; personal agents ship with hijack-class bugs; Microsoft/Google/WorkOS have released pieces but no broker.
**Competitors:** Okta/WorkOS/Casdoor, Microsoft governance toolkit, Google SAM; also NewCore ($66M) and Oak ($60M) per the investor sweep — crowded.
**Scores:** Market 8 · Timing 8 · Lab-immunity 8 · Whitespace 6 · Moat 6 · Capital eff. 8 · Demand 7.

### 4. Confidential, self-hosted agent sandbox fleet
**Pitch:** On-prem/sovereign microVM-fork sandbox fleet running inside TEEs with GPU attestation, sold as an appliance/operator to EU, India, Gulf regulated buyers.
**Why now:** Daytona closed its source; CoCo's GPU roadmap is the most-reacted issue ([#278](https://github.com/confidential-containers/confidential-containers/issues/278)); c8s notes NVIDIA GPU attestation "remains incomplete" ([repo](https://github.com/confidential-dot-ai/c8s)); OpenPCC (951 stars) proves demand for verifiable privacy ([repo](https://github.com/openpcc/openpcc)).
**Scores:** Market 7 · Timing 7 · Lab-immunity 8 · Whitespace 5 · Moat 7 · Capital eff. 4 · Demand 6.

### 5. Inference memory hierarchy: KV-cache and checkpoint storage tier
**Why now:** vLLM RFCs on KV offload (57 comments), disaggregated prefill are still open ([#19854](https://github.com/vllm-project/vllm/issues/19854), [#5557](https://github.com/vllm-project/vllm/issues/5557)); OpenLake claims 66x TTFT on 128K cached context ([repo](https://github.com/openlake-project/openlake)).
**Scores:** Market 7 · Timing 7 · Lab-immunity 7 · Whitespace 5 · Moat 6 · Capital eff. 5 · Demand 7.

### 6. Cross-harness agent FinOps control plane
**Scores:** Market 7 · Timing 8 · Lab-immunity 5 · Whitespace 3 · Moat 4 · Capital eff. 9 · Demand 9. Feature, not company, unless bundled with identity.

### 7. Edge/on-prem "fit-and-serve" for open-weight frontier MoEs
**Evidence:** llmfit 34.7k ([repo](https://github.com/AlexsJones/llmfit)), GLM-5 744B/40B-active Apache-2.0 ([repo](https://github.com/zai-org/GLM-5)).
**Scores:** Market 6 · Timing 8 · Lab-immunity 8 · Whitespace 4 · Moat 4 · Capital eff. 6 · Demand 8.

## Ranked shortlist
1. **Vertical RL-environment studio with expert graders** (avg 7.4) – labs are customers, humans are the moat.
2. **Neutral multi-silicon serving certification** (7.0) – structurally un-absorbable; capital-heavy.
3. **Agent identity/delegation broker** (7.3, lower whitespace) – best software-only bet; crowded by funded entrants.
4. **Confidential self-hosted sandbox fleet** (6.3).
5. **KV/checkpoint storage tier** (6.3).
6. **Edge fit-and-serve** (6.3).
7. **Agent FinOps alone** – feature, not company.

## Verdict
The lab-and-hyperscaler kill zone in 2026 is clearly the commodity middle: sandboxes, gateways, memory layers, and generic evals are being open-sourced by NVIDIA, Cloudflare, Tencent, Alibaba, Microsoft, and the labs themselves, and Daytona's retreat to closed source shows the pricing pressure. Durable whitespace sits where labs structurally cannot go: non-NVIDIA silicon, human-graded vertical training data, regulated on-prem attestation, and enterprise identity for agents.
