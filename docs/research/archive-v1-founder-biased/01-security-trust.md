# Sector report 1 — AI + Security / Trust / Safety

*Research agent output, 2026-09-02. Built from ~45 search result sets plus GitHub pages; most news domains were egress-blocked for full fetch, so figures come from indexed extracts and should be re-verified before external use.*

## 1. What happened in 2025–26 (money, launches, deaths)

**Money is flooding agent security, and consolidating fast.**
- AI-security startups raised $855M across 150+ seed rounds in 2026; agentic-AI security startups pulled $3.6B ([aiweekly](https://aiweekly.co/alerts/ai-security-startups-pull-855m-across-150-seed-rounds-in-2026), [softwarestrategiesblog](https://softwarestrategiesblog.com/2026/03/28/agentic-ai-security-startups-funding-mna-rsac-2026/)). AIR debuted this month with $50M (Sequoia/Greenoaks) for agent supply-chain security ([PYMNTS](https://www.pymnts.com/news/investment-tracker/2026/ai-agent-security-startup-air-raises-50-million-to-guard-enterprise-supply-chains/)); Geordie raised $30M Series A ([Fortune](https://fortune.com/2026/05/28/geordie-security-governance-ai-agents/)); Oasis $120M for non-human identity.
- Offensive AI: Horizon3 $250M at $2B+ (Aug 2026) ([SiliconANGLE](https://siliconangle.com/2026/08/03/horizon3-ai-raises-250m-2b-plus-valuation-autonomous-pentesting/)); XBOW $120M Series C at $1B+ ([XBOW](https://xbow.com/news/xbow-raises-120m-to-scale)); RunSybil $40M.
- **Standalone LLM red-teaming is dead as a category**: Promptfoo → OpenAI (Mar 2026), Lakera → Check Point, Protect AI → Palo Alto, SPLX → Zscaler, CalypsoAI → F5 ([Kosmoy](https://www.kosmoy.com/resources/blog/best-ai-red-teaming-tools-2026/)). Koi (skills/extension marketplace scanning) → Palo Alto for ~$400M, Feb 2026 ([KuppingerCole](https://www.kuppingercole.com/blog/care/palo-alto-networks-acquires-koi-securing-the-agentic-endpoint)).
- **Lab-kill events are real**: Claude Code Security (Feb 20, 2026) knocked CrowdStrike −8%, JFrog −25%, GitLab −8% ([Seeking Alpha](https://seekingalpha.com/news/4554814-cybersecurity-stocks-fall-after-anthropic-unveils-claude-code-security)). Anthropic shipped Managed Agents with sandboxing (Apr 8), OpenAI shipped Agents SDK with nine sandbox providers (Apr 15) ([TechCrunch](https://techcrunch.com/2026/04/15/openai-updates-its-agents-sdk-to-help-enterprises-build-safer-more-capable-agents/)); Anthropic then added self-hosted sandboxes and MCP tunnels ([The New Stack](https://thenewstack.io/anthropic-mcp-tunnels-sandboxes/)). Okta for AI Agents GA Apr 30; Entra Agent ID GA April ([Okta](https://www.okta.com/blog/ai/okta-ai-agents-early-access-announcement/)). Identity/sandbox/guardrail layers are being absorbed by platforms.

**The OpenClaw disaster is the defining incident.** 341 malicious ClawHub skills found in Feb, >824 by Feb 16, 1,184 by summer; 135k–245k exposed instances; 433+ CVEs; CERT-FR, BSI, CSA Singapore, Dutch AP, CNCERT all issued warnings ([Hacker News](https://thehackernews.com/2026/02/researchers-find-341-malicious-clawhub.html), [tracker](https://github.com/joylarkin/openclaw-security-news), [Unit 42](https://unit42.paloaltonetworks.com/openclaw-ai-supply-chain-risk/)). Cursor's DuneSlide (CVE-2026-50548/50549, CVSS 9.8) let zero-click prompt injection escape its sandbox ([SecurityWeek](https://www.securityweek.com/critical-cursor-ai-ide-flaws-could-lead-to-os-level-remote-code-execution/)). OX Security found a design default in every official MCP SDK exposing ~200k instances ([CSA](https://labs.cloudsecurityalliance.org/research/csa-research-note-mcp-security-crisis-20260504-csa-styled/)); 43% of public MCP servers allow command injection ([Composio](https://dev.to/composiodev/mcp-vulnerabilities-every-developer-should-know-6f9)).

**Fraud side:** seniors lost $7.75B in 2025 (+59%); AI-enabled fraud grew 1,210% YoY; $352M in AI-referenced senior losses ([HousingWire](https://www.housingwire.com/articles/fbi-seniors-cybercrime-2025/), [Ledger](https://www.ledgerapp.app/blog/fbi-ic3-2025-ai-fraud-report)). Pindrop hit $100M ARR with 5B call recordings as its moat ([Forbes](https://www.forbes.com/sites/stephenpastis/2025/04/24/this-fraud-detection-startup-made-100-million-protecting-against-deepfake-calls/)).

**Regulation/insurance:** EU AI Act high-risk and Article 50 obligations hit Aug 2, 2026, but the Digital Omnibus (May 7 agreement) pushed machine-readable marking to Dec 2, 2026 and reopened other deadlines ([Sidley](https://datamatters.sidley.com/2026/06/24/eu-ai-act-transparency-obligations-preparing-for-compliance-by-2-august-2026/), [Latham](https://www.lw.com/en/insights/ai-act-update-eu-resolves-to-change-rules-and-extend-deadlines)). Notified-body ecosystem "not ready" ([eyreACT](https://eyreact.com/notified-bodies-ai-act/)). Insurers moved from silent to explicit AI coverage/exclusions Jan 1, 2026 (ISO endorsements); Armilla/Chaucer launched "Vanguard AI" covering autonomous-agent failures (Feb 10, 2026) ([Armilla](https://www.armilla.ai/resources/armilla-launches-affirmative-ai-liability-insurance-with-lloyds-underwriter-chaucer)); HSB and YC-backed Corgi launched AI liability lines ([Munich Re](https://www.munichre.com/hsb/en/press-and-publications/press-releases/2026/2026-03-18-introducing-ai-liability-insurance-for-small-businesses.html), [Artificial Lawyer](https://www.artificiallawyer.com/2026/05/05/corgi-launches-ai-liability-insurance/)).

## 2. Practitioner sentiment (Reddit / X / conference floor)

- **AI-SOC skepticism**: a widely-cited Reddit thread ran an LLM on 348 known false positives + 1 true positive; 71% accuracy, and it missed the real incident ([Help Net Security](https://www.helpnetsecurity.com/2026/03/26/future-ai-soc-vendor-claims/)). Gartner has AI SOC Agents at the Peak of Inflated Expectations and warns of "agent washing" ([Dropzone](https://www.dropzone.ai/blog/gartner-hype-cycle-security-operations-2026)); Forrester predicts 25% of planned AI security spend deferred to 2027 ([BusinessWire](https://www.businesswire.com/news/home/20251028226928/en/Forresters-2026-Technology-Security-Predictions-As-AIs-Hype-Fades-Enterprises-Will-Defer-25-Of-Planned-AI-Spend-To-2027)).
- **Prompt injection is unsolved and everyone knows it**: OWASP's Ariel Fogel at Infosec Europe 2026 called it "unresolved" with "no reliable mechanism to enforce privilege boundaries" ([Infosecurity](https://www.infosecurity-magazine.com/news/infosec-europe-prompt-injection/)); adaptive attacks bypass essentially every published defense ([arXiv](https://arxiv.org/pdf/2505.18333)). Practitioner takeaway: stop trying to filter prompts, constrain what the agent *can do*.
- **Wrapper fatigue**: "each release resets the clock for every proprietary layer built on top" ([AI Journal](https://aijourn.com/the-wrapper-trap-is-slowing-ai-security-down/)); thin wrappers lose most users within 90 days ([HatchWorks](https://hatchworks.com/blog/gen-ai/ai-wrapper-product-strategy/)).
- **Offensive researchers are angry at the labs**: guardrails on frontier models frustrate security researchers (TechCrunch, Jun 10); "you spend a lot of time negotiating with the model instead of working on the core security program" (Jul 23); OpenAI revoked researchers' Trusted Access for Cyber (Aug 19) ([TechCrunch](https://techcrunch.com/2026/08/19/researchers-complain-that-openai-revoked-their-access-to-limited-cyber-program/), [TechCrunch](https://techcrunch.com/2026/07/23/how-ai-guardrails-are-impeding-the-work-of-offensive-cybersecurity-researchers/)).
- **Practitioners want to see what agents do, not black boxes**: "hallucinations and black-box behavior... especially as agents start taking actions" ([SiliconANGLE](https://siliconangle.com/2026/03/21/rsac-2026-preview-ai-hype-meets-operating-model-reality/)).

## 3. Startup ideas

### Idea A — Kernel-level "EDR for agents": eBPF/LSM enforcement for locally-running agents (OpenClaw, Claude Code, Cursor, Codex)
- **Pitch**: A Rust + eBPF daemon that enforces per-agent, per-tool-call syscall/file/network policy at the kernel, independent of which agent or model is running.
- **Customer**: Security teams at companies whose devs run coding agents on laptops/CI; MSPs; OpenClaw-style self-hosters.
- **Why now**: DuneSlide showed app-level sandboxes fail; OpenClaw shows agents run on endpoints outside any lab's control. ARMO frames "what eBPF catches and misses" ([ARMO](https://www.armosec.io/blog/ebpf-based-ai-agent-enforcement/)); Meta is open-sourcing BpfJailer for AI workloads; a Rust+eBPF tool ("Busted") already intercepts LLM traffic via OpenSSL uprobes ([awesome-ebpf](https://github.com/qmonnet/awesome-ebpf)).
- **Competition**: Sysdig (agent runtime security, RSAC 2026), ARMO, Anthropic sandbox-runtime (bubblewrap/seatbelt), Docker Sandboxes. Crowded at the *cloud* layer, thin at the *endpoint* layer.
- **Why labs won't kill it**: Anthropic's sandbox only protects Anthropic's agent and explicitly doesn't stop credential exfiltration when permissions are skipped ([Anthropic](https://anthropic.com/engineering/claude-code-sandboxing)). No lab will ship kernel enforcement for competitors' or open-source agents.
- **Moat**: OS-level integration, policy corpus, vendor-neutrality.
- **Founder fit**: Ideal (Rust, kernel, systems).
- **Scores**: Market 7 · Timing 9 · Defensibility 7 · Fit 10 · Capital 9.

### Idea B — Offensive-security model infrastructure: GPU-optimized on-prem runtime for uncensored open-weight pentest agents
- **Pitch**: Turnkey local inference + agent harness (Rust, custom CUDA kernels) for pentest firms/MSSPs that the labs are refusing to serve.
- **Customer**: Pentest boutiques, red teams, MSSPs, defense contractors.
- **Why now**: Labs are actively retreating (guardrails, revoked TAC access); open-weight models are good enough ([NPR](https://www.npr.org/2026/05/31/nx-s1-5816391/ai-safety-concerts-danger-open-weight-models-risks)); Horizon3/XBOW prove demand but are enterprise-priced.
- **Competition**: Horizon3, XBOW, RunSybil (all frontier-model-dependent), open-source CAI ([arXiv](https://arxiv.org/abs/2508.21669v1)).
- **Why labs won't kill it**: They *can't* — their policy is to restrict this.
- **Moat**: Legal/vetting process (KYC of customers), tuned models, hardware bundles.
- **Risks**: Dual-use liability; you become the "uncensored" vendor.
- **Scores**: Market 6 · Timing 8 · Defensibility 9 · Fit 9 · Capital 6.

### Idea C — Agent flight recorder: tamper-evident action logs sold to insurers and EU AI Act auditors
- **Pitch**: A vendor-neutral, cryptographically chained (Merkle/append-only) record of every agent tool call, input, and approval — the evidence layer insurers and regulators will demand.
- **Customer**: Enterprises buying AI liability cover; AI vendors seeking Armilla/Chaucer/HSB/Corgi policies; high-risk AI providers needing Article 12 logging.
- **Why now**: Insurers moved to explicit AI coverage Jan 2026 and price on "containment and monitoring controls" ([Compass ITC](https://www.compassitc.com/blog/old-policies-new-technology-is-your-insurance-actually-ready-for-ai)); Armilla underwrites via governance + technical assessment; EU logging obligations bite Aug 2026/2027.
- **Competition**: Observability vendors (Langfuse etc.), Geordie, Anthropic tracing. None are insurer-facing or vendor-neutral.
- **Why labs won't kill it**: An insurer won't accept a self-attested trace from the party whose model caused the loss; neutrality is the product.
- **Moat**: Trust relationships with carriers/MGAs, becomes de-facto evidence standard (network effect).
- **Founder fit**: High on tech; needs one insurance-savvy partner.
- **Scores**: Market 7 · Timing 8 · Defensibility 8 · Fit 7 · Capital 8.

### Idea D — "Let's Encrypt for C2PA" + signing appliance for the EU Article 50 marking deadline
- **Pitch**: Free/cheap C2PA certificates and a hardened Rust signing service/HSM appliance so the long tail of EU deployers can machine-mark synthetic media by Dec 2, 2026.
- **Customer**: EU media companies, ad agencies, open-weight model deployers, camera/device OEMs.
- **Why now**: Marking obligation now due Dec 2, 2026 ([Sidley](https://datamatters.sidley.com/2026/06/24/eu-ai-act-transparency-obligations-preparing-for-compliance-by-2-august-2026/)); trust-list certs cost ~$289/yr from DigiCert with no free CA ([EyeSift](https://www.eyesift.com/faq/c2pa-content-credentials-2026-cryptographic-provenance-adoption/)); Pixel 10 and Galaxy S25 sign natively so the rails exist.
- **Competition**: Truepic, DigiCert, Adobe CAI. Few and slow.
- **Why labs won't kill it**: Labs watermark their *own* outputs (SynthID, OpenAI C2PA); they won't run a public CA or sign for open-weight deployers.
- **Moat**: Regulatory, trust-list membership, standards-body position.
- **Risk**: Standards politics; monetization unclear.
- **Scores**: Market 5 · Timing 8 · Defensibility 8 · Fit 7 · Capital 8.

### Idea E — Hardware scam-call interceptor for seniors, sold through banks/credit unions and senior-living operators
- **Pitch**: A small on-prem device (or router firmware) running local voice-clone/scam detection that pauses the call and pings a family member before money moves.
- **Customer**: Adult children of seniors; credit unions (liability for elder fraud); senior-living operators.
- **Why now**: $7.75B senior losses, AI fraud +1,210% ([HousingWire](https://www.housingwire.com/articles/fbi-seniors-cybercrime-2025/)); FCC made AI-voice robocalls illegal but STIR/SHAKEN verifies numbers, not speakers ([FCC](https://www.fcc.gov/document/fcc-makes-ai-generated-voices-robocalls-illegal)).
- **Competition**: App-only (SeniorShield, ZoraSafe); Pindrop is enterprise-only with a 5B-call data moat.
- **Why labs won't kill it**: Physical device, senior-care distribution, family trust relationships.
- **Moat**: Hardware + channel; local inference means no cloud lab dependency.
- **Fit**: Good on embedded/GPU; weak on consumer distribution; slower, capital-heavier.
- **Scores**: Market 8 · Timing 8 · Defensibility 9 · Fit 5 · Capital 4.

### Idea F — Open agent-skill/MCP registry scanner with signed attestations ("Sigstore for skills")
- **Pitch**: Static + dynamic analysis of skills/MCP servers with signed provenance, published as an open registry.
- **Evidence**: 1,184 malicious skills; 36.7% of 7k MCP servers SSRF-vulnerable ([Practical DevSecOps](https://www.practical-devsecops.com/mcp-security-statistics-2026-report/)).
- **Crowded**: Koi ($400M exit), AIR ($50M), Backslash, plus 22 open-source gateways ([awesome-mcp-gateways](https://github.com/e2b-dev/awesome-mcp-gateways)). Anthropic owns the MCP registry and could add scanning tomorrow.
- **Scores**: Market 7 · Timing 7 · Defensibility 4 · Fit 9 · Capital 9.

## 4. Ranked shortlist

| Rank | Idea | Total /50 | Why |
|---|---|---|---|
| 1 | **A. Kernel-level agent enforcement** | 42 | Perfect founder fit, hottest incident stream, labs structurally can't cover third-party agents |
| 2 | **C. Agent flight recorder for insurers/regulators** | 38 | Neutral evidence layer; regulation + insurance pull; stackable on A |
| 3 | **B. Offensive-security model runtime** | 38 | Labs are exiting the market; highest defensibility, but dual-use risk |
| 4 | **D. Let's Encrypt for C2PA** | 36 | Regulatory deadline, low competition, unclear revenue |
| 5 | **E. Senior scam hardware** | 34 | Biggest human need; wrong founder for distribution |
| 6 | **F. Skill registry scanner** | 36 | Great tech fit, but Koi/AIR/Anthropic make it a feature |

**Verdict.** The sector has money and real pain, but anything shaped like "guardrail / red-team / identity / sandbox *for the lab's own agent*" is being acquired or shipped natively within 12 months, so a solo founder must anchor outside the model: the kernel, the endpoint, the insurer, the regulator, or the physical phone line. The strongest fit for a Rust/systems/GPU founder is kernel-level enforcement and evidence logging for agents the labs don't control (open-source, multi-vendor, on-prem), with the insurer/EU-audit channel as the paying customer rather than the developer. Avoid detection-only plays (deepfake classifiers, AI SOC triage) where data-rich incumbents and hype-cycle skepticism both work against you.
