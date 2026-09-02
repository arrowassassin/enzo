# Sector report 6 — AI infrastructure "picks and shovels" labs won't build

*Research agent output, 2026-09-02. 22 web searches plus ~30 page fetches. Reddit, HN, TechCrunch, Substack, arXiv and most blogs were blocked by the egress proxy, so practitioner sentiment is sourced from GitHub discussions/issue trackers and secondary write-ups. Treat sentiment claims as directional.*

## 1. Market signal (2025–26)

**Money is flooding the layer, and it concentrates on hosted inference + sandboxes:**
- Baseten: $1.5B Series F at ~$13B, ~$600M ARR — https://valueaddvc.com/pulse/baseten-1-5b-series-f-inference-2026
- Fireworks: $1.5B Series D at $17.5B (Jul 2026) — https://valueaddvc.com/blog/fireworks-ai-valuation-2026-17-5b-series-d-and-the-1b-arr-inference-business
- Modal: $355M Series C at $4.65B, ~$300M run-rate — https://siliconangle.com/2026/05/21/serverless-ai-infrastructure-startup-modal-labs-seals-355m-funding-round/
- Prime Intellect: $130M Series A at $1B, 6,000 customers, >$100M annualized (open-stack compute + RL environments + sandboxes) — https://www.pymnts.com/news/investment-tracker/2026/prime-intellect-raises-130-million-to-help-companies-train-ai-agents/
- Nous Research: $75M Series B at $1.5B (Jul 2026) — https://app.dealroom.co/news/note/nous-research-raises-75m-series-b-at-1-5b-valuation
- OpenRouter: $113M Series B at $1.3B (May 2026), then Stripe reportedly acquiring for >$7B — https://www.techtimes.com/articles/317353/20260529/ai-gateway-openrouter-raises-113m-google-nvidia-route-between-their-models.htm
- Daytona: $24M Series A (Feb 2026), **went closed-source June 2026** — https://northflank.com/blog/daytona-vs-e2b-ai-code-execution-sandboxes
- Braintrust: $80M Series B at $800M; observability TAM ~$2.69B — https://latitude.so/blog/best-llm-observability-tools-agents-latitude-vs-langfuse-langsmith
- Kernel-gen seed rounds: Mako $8.5M (AMD + Tenstorrent partnerships) — https://app.dealroom.co/news/feed/mako-secures-8-5m-seed-funding ; Luminal $5.3M — https://www.startupresearcher.com/news/luminal-raises-usd5-3-million-for-faster-ai-inference
- Unsloth: only ~$500K raised (YC) despite huge OSS usage — https://getlatka.com/companies/unsloth.ai#funding

**Exits/absorptions/deaths (the cautionary list):**
- Nvidia–Groq $20B license + ~90% staff (Dec 2025); Groq 3 LPX in production Aug 2026 — https://www.cnbc.com/2026/08/24/nvidia-says-groq-racks-will-be-online-this-year-after-20-billion-deal.html ; Cerebras IPO on $20B OpenAI commitments — https://hashrateindex.com/blog/independent-ai-chip-companies-ai-asic-market-part-3/ ; Qualcomm in talks for Tenstorrent at $8–10B — https://www.techtimes.com/articles/319017/20260624/qualcomm-bets-14-billion-cracking-nvidias-ai-monopoly-risc-v-open-compiler.htm
- Inference-as-a-service graveyard: Modelbit and Ploomber shut down; BentoML, Replicate, Lepton acquired — https://faizank.substack.com/p/inference-as-a-service
- Koyeb Sandboxes folded into Mistral — https://github.com/restyler/awesome-sandbox
- Labs buying dev-infra: Anthropic bought Stainless; DeepMind licence-hired Contextual AI — https://www.startuphub.ai/ai-news/ai-news/2026/four-labs-four-acquisitions-ai-consolidation-may-2026 ; >$20B in "reverse acqui-hires" since 2024 — https://www.fastaijobs.com/career-hacks/ai-acquihire-map-2026
- DePIN compute is contracting: Akash ~334 GPUs listed, 84 in use, −57% QoQ — https://www.buildmvpfast.com/blog/depin-gpu-inference-io-net-akash-below-aws-2026

## 2. What labs already do (honest baseline)
- Anthropic open-sourced `sandbox-runtime` (Seatbelt/bubblewrap + network proxy, "Beta Research Preview", explicitly documents domain-fronting and docker.sock bypasses; cloud sessions use real microVMs) — https://github.com/anthropic-experimental/sandbox-runtime
- Google owns AP2 (60+ partners), Coinbase owns x402 (165M+ tx, adopted by AWS AgentCore and Stripe) — https://atxp.ai/blog/agent-payment-protocols-compared/
- Labs ship evals/tracing and buy SDK/gateway plumbing (Stainless). Anything that is "a better wrapper around our API" is at risk.

**What they structurally won't do:** run inside your air-gapped rack, optimise Qwen/Mistral/gpt-oss on MI300/Tenstorrent, meter competitors' tokens neutrally, or hold your secrets. Those are the only safe zones.

## 3. Practitioner pain (what could be verified)
- Ollama collapses to ~41 tok/s under concurrent load vs vLLM ~793 (Red Hat benchmark) — enterprises graduate from "easy" to "hard" tooling with nothing in between — https://pub.towardsai.net/vllm-vs-ollama-vs-llama-cpp-vs-sglang-ollama-collapses-to-41-tokens-under-load-8f7a850d1e07
- llama.cpp discussions: multi-GPU NCCL gains elusive, ROCm tensor-split issues, Vulkan/Intel SYCL perf, quant-quality confusion — https://github.com/ggml-org/llama.cpp/discussions
- Sandbox catalog gaps: GPU/CUDA inside sandboxes, persistence across restarts, Windows, policy enforcement at scale — https://github.com/restyler/awesome-sandbox ; "Ask HN: best microVMs for AI agents?" asks for self-hosted options — https://news.ycombinator.com/item?id=46450931
- Enterprise cost pain: 73% exceeded AI cost plans, agentic projects overshoot 2.4x; Linux Foundation launched a Tokenomics Foundation at FinOps X 2026 — https://www.usu.com/en/blog/recap-finopsx-san-diego-2026 ; https://www.ciodive.com/news/foundation-tackle-ai-token-cost-management/822839/
- Sovereignty: EU AI Act / India DPDP push data-stays-local; Nvidia sovereign revenue >$30B; Gartner sovereign cloud IaaS $80B in 2026 — https://www.intellabel.com/sovereign-ai-infrastructure-why-on-prem-is-coming-back-in-2026 ; Mistral building regional endpoints + EU compute coalition — https://mistral.ai/news/regional-inference-open-models-new-compute/
- Rust inference exists but is un-commercialised: mistral.rs 7.6k stars, CUDA/Metal/ROCm, ISQ quantization, no company behind it — https://github.com/EricLBuehler/mistral.rs

## 4. Ideas

### A. Sovereign inference appliance (Rust, single binary, NVIDIA + AMD + Apple)
- **Pitch:** vLLM-class throughput with llama.cpp-class operability, shipped as one signed binary for air-gapped/regulated deployments; paid support + hardware certification.
- **Customer:** EU/India/GCC mid-size regulated firms, sovereign clouds, MSPs, defence integrators.
- **Why now:** open-weight quality (Qwen/Mistral/gpt-oss), ROCm 10.0 maturity — https://rocm.blogs.amd.com/ecosystems-and-partners/rocm-x-blog/README.html , sovereignty spend above.
- **Competitors:** vLLM (Red Hat), SGLang, Ollama, LM Studio, HF Endpoints, mistral.rs; Baseten/Fireworks are cloud-first. Crowded on the engine, thin on "supported on-prem product".
- **Lab-proof:** labs monetise hosted APIs; an OpenAI on-prem engine for Qwen is implausible.
- **Moat:** AMD/Intel/Apple kernel work, certification matrix, support relationships; OSS core + paid enterprise binary (LiteLLM model).
- **Feasibility:** high; fork/contribute to mistral.rs or start from candle. Risk: Red Hat gives vLLM away with RHEL AI.
- **Scores:** market 8 · timing 8 · defensibility 8 · founder fit 9 · capital 7

### B. BYOC/on-prem microVM sandbox fleet with GPU passthrough
- **Pitch:** "E2B you can run in your own VPC or rack": Rust control plane over Firecracker/libkrun (both Rust), snapshot/restore, GPU-in-sandbox, per-tool-call egress policy.
- **Customer:** agent startups and banks that cannot send code to E2B; RL-environment builders.
- **Why now:** Daytona closed-sourced, E2B self-host is GCP-only, K8s SIG agent-sandbox is v0.1 — https://github.com/kubernetes-sigs/agent-sandbox ; microsandbox (Rust, 8.1k stars, YC) proves demand — https://github.com/microsandbox/microsandbox
- **Competitors:** very crowded (E2B, Modal, Vercel, Deno, Fly, Docker Sandboxes, Blaxel, Runloop, NVIDIA OpenShell, nono 3.9k stars from Sigstore team — https://github.com/nolabs-ai/nono ).
- **Lab-proof:** Anthropic's runtime is OS-level and Claude-Code-scoped; labs won't operate your on-prem fleet.
- **Moat:** modest — GPU isolation + snapshotting is hard but copyable; win on regulated BYOC.
- **Scores:** market 7 · timing 6 · defensibility 6 · founder fit 9 · capital 7

### C. Non-NVIDIA kernel/compiler layer for open-weight inference (AMD, Tenstorrent, Intel)
- **Pitch:** open kernel library + porting service that makes the top-20 open models hit spec on MI300/MI400 and Wormhole/Blackhole; sell to chip vendors, neoclouds, sovereign DCs.
- **Why now:** TT-Forge in beta, TT-Lang "early preview", multi-chip only via XLA — https://github.com/tenstorrent/tt-forge ; Qualcomm/Tenstorrent, AMD/Mako partnerships show vendors pay for ecosystem — https://www.m13.co/article/meet-makora-unlock-peak-gpu-performance-and-reduce-ai-inference-costs-automatically
- **Competitors:** Mako, Luminal, vendor in-house teams. Not crowded.
- **Lab-proof:** absolutely — labs optimise for their own fleets, not your MI300.
- **Moat:** rare skill, benchmark reputation; risk is customer concentration and the Groq-style absorb-the-team outcome (likely a good exit for a solo founder, not a standalone company).
- **Scores:** market 6 · timing 8 · defensibility 9 · founder fit 10 · capital 8

### D. Agent egress + credential + spend enforcement proxy (self-hosted Rust)
- **Pitch:** one binary that sits between agents and the world: default-deny egress, credential injection, per-agent token/USD budgets, neutral multi-vendor metering feeding FinOps.
- **Why now:** Tokenomics Foundation, 1Password entering AI spend — https://www.usage.ai/blogs/finops/ai-ml-cost/finops-x-2026-takeaways ; primitives exist but are fragmented (iron-proxy, Infisical agent-vault — https://github.com/Infisical/agent-vault ).
- **Competitors:** LiteLLM (57.8k stars, dual licence, cost tracking) — https://github.com/BerriAI/litellm ; OpenRouter/Stripe; Cloudflare/Vercel gateways. Gateway layer is crowded; sandbox-side enforcement is not.
- **Lab-proof:** a lab won't meter rivals' tokens or broker your secrets.
- **Moat:** low-medium; becomes a feature of LiteLLM/Infisical unless tied to B.
- **Scores:** market 7 · timing 8 · defensibility 6 · founder fit 7 · capital 9

### E. x402 metering/facilitator for self-hosted inference and MCP servers
- **Pitch:** pay-per-request billing for on-prem/open-weight endpoints via x402, with the usage-metering piece nobody has solved — https://usagebox.com/articles/ai-agent-payment-stack-2026-x402-ap2-agent-pay-metering-gap
- **Why now:** 100M+ tx on Base, AWS/Stripe adoption — https://www.chainalysis.com/blog/x402-agentic-payments-adoption/
- **Competitors:** Coinbase, Stripe, ATXP, dozens of crypto teams. Protocol owners control the rail.
- **Lab-proof:** yes, but rail owners (Coinbase/Stripe/Google) are the killers instead.
- **Founder fit:** low (fintech/compliance, not GPU work).
- **Scores:** market 6 · timing 7 · defensibility 4 · founder fit 4 · capital 8

**Rejected:** agent observability/evals (crowded, labs build it), decentralised GPU marketplaces (supply collapsing), COBOL migration (IBM watsonx, Hypercubic $5.3M — https://www.hypercubic.ai/insights/hypercubic-raises-usd5-3m-to-build-the-future-of-mainframe-modernization — and labs' models keep eating it), datacenter cooling (hardware-capital game; $5.2B raised in one month by Nexthop/Frore etc. — https://www.buildmvpfast.com/blog/ai-infrastructure-funding-cooling-networking-startups-2026 ).

## 5. Ranked shortlist
1. **A — Sovereign Rust inference appliance** (best business; highest lab-immunity; wedge via AMD/Apple support and EU/India compliance).
2. **C — Non-NVIDIA kernel layer** (best founder fit and defensibility; likely ends in a vendor acqui-hire, which may be the right outcome for a solo founder).
3. **B + D combined — on-prem sandbox fleet with built-in egress/credential/spend policy** (real gap, but only defensible bundled and aimed at regulated buyers).
4. E — agent payments metering (interesting, wrong founder).

**Verdict:** This is the highest-fit sector for a Rust/GPU systems founder, and 2026 evidence shows buyers paying at scale — but the money is flowing to hosted, capital-heavy plays (Baseten, Fireworks, Modal) while the cheap, lab-proof territory is on-prem, sovereign and non-NVIDIA. The credible solo path is open-core: a performance-leading OSS engine that earns distribution, monetised through certified enterprise binaries and support for regulated/hardware-specific deployments, not a SaaS. The main risk is not a lab feature launch but Red Hat/AMD/Nvidia giving the same thing away or absorbing the team — plan the company so that outcome is acceptable.
