# Sector report 3 — Physical AI / Robotics / Edge & Embedded

*Research agent output, 2026-09-02. 21 web searches + ~15 page fetches. Reddit and X were blocked by the egress proxy, so practitioner sentiment is proxied via GitHub issue trackers (LeRobot, Rockchip RKNN, Hailo) and secondary coverage.*

## 1. State of the sector (2025–2026)

**Capital is concentrated at the top of the stack.**
- Skild AI closed a $1.4B Series C at $14B (Jan 2026, SoftBank/NVentures/Bezos/Samsung) — [sacra.com](https://sacra.com/c/skild-ai/), [aifundingtracker](https://aifundingtracker.com/top-humanoid-robotics-startups-funded/). Figure hit $39B post-money (Sept 2025). Physical Intelligence raised $470M and is reportedly in talks for $1B at $11B+ — [newmarketpitch](https://newmarketpitch.com/blogs/news/physical-ai-top-startups-fundraising).
- 1X NEO: $20K / $499/mo, 10,000+ pre-orders, but no verified customer delivery as of July 2026; 60–70% autonomy target with human teleoperators filling the gap, and privacy backlash on "Expert Mode" — [theplanettools.ai](https://theplanettools.ai/blog/1x-neo-first-consumer-humanoid-dated-priced-teleop-caveat-may-2026), [Engadget](https://www.engadget.com/ai/1x-neo-is-a-20000-home-robot-that-will-learn-chores-via-teleoperation-040252200.html).
- Shakeout is underway: UBS forecasts only ~30K humanoids shipped in 2026; Chinese analysts describe a "cruel elimination race" — [36kr](https://eu.36kr.com/en/p/3657117189890433), [ahr.so](https://ahr.so/the-first-wave-of-humanoid-robot-failures-has-arrived-and-its-a-warning/). Guardian Agriculture (spray drones) shut down Aug 2025 — [Robot Report](https://www.therobotreport.com/top-10-robotics-developments-of-september-2025/).
- Robotics *software* captures only ~1.59% of category capital; Foxglove's $40M Series B (Nov 2025, Bessemer) is the standout — [newmarketpitch](https://newmarketpitch.com/blogs/news/robotics-software-funding-analysis), [BusinessWire](https://www.businesswire.com/news/home/20251112126106/en/Foxglove-Raises-$40-Million-Series-B-to-Power-the-Future-of-Physical-AI). InOrbit is open-sourcing its fleet manager (OpenRobOps) later in 2026 — [Robotics 24/7](https://www.robotics247.com/article/inorbit-unveils-openrobops-open-source-fleet-manager-platform).

**Open models are now the default; the bottleneck moved to data and deployment.**
- openpi (π0, π0-FAST, π0.5) is Apache-2.0, 13.6K stars; inference needs an 8GB+ NVIDIA GPU, LoRA fine-tune 22.5GB+, full 70GB+; several features JAX-only — [github.com/Physical-Intelligence/openpi](https://github.com/Physical-Intelligence/openpi). LeRobot: 27.1K stars, 356 open issues, Python/PyTorch, supports ACT/SmolVLA/π0.5/GR00T N1.7 — [github.com/huggingface/lerobot](https://github.com/huggingface/lerobot). 1,222 public LeRobot datasets from 377 users were curated into MolmoAct2's 38K-episode dataset — [arxiv 2605.02881](https://arxiv.org/pdf/2605.02881).
- NVIDIA's Isaac GR00T reference humanoid (Unitree H2 Plus + Jetson Thor + Isaac Teleop/Sim/ROS) ships late 2026 — [NVIDIA newsroom](https://nvidianews.nvidia.com/news/nvidia-open-humanoid-robot-reference-design).
- "Data is the bottleneck, not compute": teleop labor ≈ $40/hr, ~30 usable episodes/hr, ≈$67K per 50K episodes before QA — [dexset.ai](https://dexset.ai/blogs/teleoperation-data-collection-robot-learning-complete-2026/); XDOF is already paid by labs to collect it — [TechCrunch, Jun 2026](https://techcrunch.com/2026/06/17/collecting-robot-training-data-is-dirty-unglamorous-work-some-ai-labs-are-already-paying-xdof-to-do-it/); [SCSP](https://scsp222.substack.com/p/isf-voices-2026-the-robotics-data).

**Edge silicon is cheap and fragmented; toolchains are the pain.**
- Raspberry Pi AI HAT+ 2 (Hailo-10H, 8GB) is $130, ~30–50 tok/s on Llama 3.2 1B — [Tom's Hardware](https://www.tomshardware.com/raspberry-pi/raspberry-pi-ai-hat-plus-2-review); Jetson Orin Nano Super $249 — [Notebookcheck](https://www.notebookcheck.net/The-Nvidia-Jetson-Orin-Nano-Super-a-powerful-generative-AI-SBC-is-now-available-worldwide-for-249.934029.0.html); Jetson T4000 and TensorRT Edge-LLM launched Jan 2026 — [NVIDIA blog](https://developer.nvidia.com/blog/tag/jetson/page/3).
- Edge-AI startups raised ~$2.55B over 21 deals (Aug 2025–Jul 2026); Quadric $30M Series C with revenue tripling; Acrab out of stealth with $350M+ — [newmarketpitch](https://newmarketpitch.com/blogs/news/edge-ai-funding-analysis), [Edge AI & Vision](https://www.edge-ai-vision.com/2026/01/quadric-inference-engine-for-on-device-ai-chips-raises-30m-series-c-as-design-wins-accelerate-across-edge/), [SemiEngineering](https://semiengineering.com/startup-funding-q2-2026/).
- Rust runtimes: candle 21K stars (CUDA/Metal/WASM, no NPU), burn 15.9K (CUDA/ROCm/Vulkan/WGPU/no_std, ONNX→Rust), tract 3.1K (Sonos, ARM CPU/Metal/CUDA/WASM, no NPU), mistral.rs 7.6K (CUDA/Metal/CPU, no embedded) — [candle](https://github.com/huggingface/candle), [burn](https://github.com/tracel-ai/burn), [tract](https://github.com/sonos/tract), [mistral.rs](https://github.com/EricLBuehler/mistral.rs). **None target NPUs.**

**Regulation:** EU CRA incident/vulnerability reporting became mandatory Sept 11, 2026; full technical requirements Dec 11, 2027. AI Act high-risk pushed to Dec 2027 / Aug 2028; robotics falls partly under the Machinery Regulation via a delegated act due Aug 2028 — [eeNews Europe](https://www.eenewseurope.com/en/cyber-resilience-act-part-1/), [TechTimes](https://www.techtimes.com/articles/326249/20260901/eu-cyber-resilience-act-reporting-deadline-filing-portal-launches-same-day-it-becomes-mandatory.htm), [Travers Smith](https://www.traverssmith.com/knowledge/knowledge-container/eu-agrees-to-delay-key-ai-act-compliance-deadlines/).

**Glasses:** Meta opened Ray-Ban Display to devs May 19, 2026 (phone-hosted mobile SDK + web apps; no on-glasses code) — [Meta](https://developers.meta.com/blog/build-for-display-glasses/), [Auganix](https://www.auganix.org/ar-news-ray-ban-display-developer-preview/). Even Realities launched Even Hub (Apr 3, 2026; ~50 apps, 2,000 devs) — [AndroidGuys](https://androidguys.com/news/even-realities-even-hub-app-store-for-g2-smart-glasses-launches/). MentraOS (MIT, 2.3K stars, TypeScript apps run on phone) supports G1/G2/Vuzix/Mentra Live — [GitHub](https://github.com/Mentra-Community/MentraOS).

## 2. Practitioner pain (evidence)

- **LeRobot issues (Aug 2026):** dataset converters rejecting 3.1M unified actions (#4406) and 92% of RoboTwin actions (#4404); request for a CLI to pin camera mount poses (#4496); explicit motor-to-URDF mapping (#4442); stale dataset revision tags (#4017); SO-101 calibration broken after upgrade (#3193); **SmolVLA produces wrong results on Jetson Orin ARM64** (#3636); torch.compile warmup crashes vs 500ms camera watchdog (#4115); an open RFC on ROS 2 strategy (#4368) — [issues](https://github.com/huggingface/lerobot/issues).
- **NPU SDK hell:** Rockchip rknn-toolkit2 has 459 open issues (unsupported ops, conversion failures, post-quant accuracy drops) — [GitHub](https://github.com/airockchip/rknn-toolkit2/issues). Hailo on Pi 5: "apt upgrade wipes user models, installs wrong runtime", "python binding incompatible with libhailort 5.x", SRAM not released between model loads on Hailo-10H — [GitHub](https://github.com/hailo-ai/hailo-rpi5-examples/issues).
- **VLA on edge is slow:** LiteVLA-Edge reaches 6.6 Hz on Orin via llama.cpp GGUF; TIDAL ~9 Hz vs 2.4 Hz baselines; a "vla.cpp" unified runtime paper appeared June 2026 — [LiteVLA-Edge](https://arxiv.org/html/2603.03380v1), [TIDAL](https://arxiv.org/html/2601.14945v2), [vla.cpp](https://arxiv.org/pdf/2606.08094), [Jetson-PI](https://arxiv.org/html/2607.12659v3).
- **Local voice:** HA's 2026 stack (faster-whisper/Piper/Ollama) gets 2–4s round-trips on GPU; openWakeWord false positives remain the top complaint; turnkey private appliances are sparse (ClawBox €549 on Orin Nano is nearly the only priced one) — [maloyan.xyz](https://maloyan.xyz/blog/home-assistant-local-voice-stack-2026), [homeautocentral](https://homeautocentral.com/speed-up-local-voice-assistant/), [gist](https://gist.github.com/yalexx/9549512b80dc9519a01f5e45ff07440b).

## 3. Startup ideas

Scores: **Mkt / Timing / Def-vs-labs / Founder fit / Capital (10 = low)**

### A. "policy.rs" — a Rust real-time runtime for deploying VLA/imitation policies on Jetson/ARM/NPU
- **Pitch:** One static binary that loads LeRobot/openpi checkpoints (ACT, SmolVLA, π0.5, GR00T-N) and runs them at deterministic control rates with async action-chunking, no Python, targeting Orin/Thor, Hailo-10H, RK3588.
- **Customer:** robotics startups and OEMs moving from lab (Python) to product (embedded); the 377+ LeRobot dataset publishers as a funnel.
- **Why now:** open VLA weights everywhere; edge hardware $130–$249; LeRobot's ARM64 correctness bug and Python-only inference; "vla.cpp" shows the gap is recognized but unfilled in Rust with real-time guarantees.
- **Competitors:** vla.cpp (academic), TensorRT Edge-LLM (NVIDIA-only), LeRobot async inference (Python), llama.cpp hacks. Not crowded.
- **Why labs won't kill it:** labs ship weights, not BSP-level embedded runtimes for other people's silicon; the value is in the hard real-time loop, calibration/URDF plumbing and NPU quirks, not the model.
- **Moat:** per-embodiment kernels and vendor-SDK workarounds accumulate; becomes the "deployment target" LeRobot/openpi recommend.
- **Feasibility:** ideal Rust/GPU/real-time profile; solo-buildable MVP on Orin + SO-101 in 3–4 months.
- **Risk:** NVIDIA folds it into Isaac ROS; mitigate by being cross-vendor (Hailo/Rockchip) and OSS-core.
- **Scores:** 6 / 9 / 8 / 10 / 9

### B. Robot-demo data QA & conversion CLI ("lint + Great Expectations for teleop datasets")
- **Pitch:** Local, GPU-accelerated Rust tool that validates camera/joint sync, calibration drift, duplicate/idle episodes, camera pose consistency, and converts losslessly between LeRobot v3, RLDS/DROID and lab-specific formats — with a paid hosted dashboard for data-collection vendors.
- **Customer:** data-collection shops (XDOF, Dexset, DataX), robotics teams paying ~$67K per 50K episodes and losing a chunk to QA rejects.
- **Why now:** labs are outsourcing collection; LeRobot's own converters are rejecting millions of actions; dataset format churn (v2→v3).
- **Competitors:** Foxglove ($58M raised, visualization/observability), Claru, Hugging Face itself (could add lint to LeRobot). Moderately crowded on viz, empty on validation.
- **Why labs won't kill it:** their incentive is to *buy* clean data, not build vendor tooling; QA is grounded in physical rigs.
- **Moat:** becomes the acceptance standard between buyers and collectors (spec + certification); network effects across formats.
- **Feasibility:** high; the founder's terminal/GPU-video-decode skills fit; sales are to a small, reachable set of vendors.
- **Scores:** 5 / 8 / 8 / 8 / 9

### C. Cross-NPU deployment layer in Rust (Hailo / Rockchip / Jetson / Pi 6 NPU) with fleet-safe OTA
- **Pitch:** "tract/burn with NPU backends": compile ONNX once, ship a tiny runtime that survives `apt upgrade`, with signed model OTA and rollback.
- **Customer:** industrial vision, ag-drone and smart-camera OEMs on RK3588/Hailo; Pi-based inspection integrators.
- **Why now:** 459 open RKNN issues, Hailo runtime breakage on Pi, Pi 6 NPU due Q4 2026 ([justoborn](https://justoborn.com/raspberry-pi-6/)); edge AI funding at $2.55B/yr; >70% of manufacturers plan AI visual inspection within 18 months — [IIoT World](https://www.iiot-world.com/smart-manufacturing/ai-vision-quality-manufacturing-2026/).
- **Competitors:** Quadric (IP, well funded), ONNX Runtime EPs, vendor SDKs, Edge Impulse-style platforms. Crowded at the chip-IP layer, thin at the "portable Rust runtime" layer.
- **Why labs won't kill it:** frontier labs will never write Rockchip kernels; this is silicon-specific grunt work.
- **Moat:** kernel/vendor-quirk library; certification with silicon vendors; CRA-compliant update channel (see D) as a lock-in.
- **Feasibility:** hardest technically (closed Dataflow Compilers; Hailo requires vendor access); realistic if you start with one vendor.
- **Scores:** 7 / 8 / 9 / 9 / 8

### D. CRA/AI-Act compliance kit for embedded & robotics products (SBOM, vuln reporting, secure OTA)
- **Pitch:** Rust agent + service that generates firmware SBOMs, monitors vulns, files EU reports (mandatory since 2026-09-11), and provides audited update logs for robots/IoT.
- **Customer:** EU SMB robot/IoT makers facing Dec 2027 full CRA obligations and Machinery-Regulation AI rules.
- **Why now:** the reporting portal launched the same day it became mandatory; small OEMs are unprepared.
- **Competitors:** generic SBOM vendors (not deeply verified in this pass); few robotics-specific.
- **Why labs won't kill it:** regulation-driven, hardware-scoped, relationship sales.
- **Moat:** regulatory know-how + integration into build/OTA pipelines. Weakest founder fit (sales-heavy).
- **Scores:** 6 / 9 / 10 / 5 / 8

### E. Vertical smart-glasses workflow apps on MentraOS/Even Hub (e.g., factory QA checklists, warehouse pick guidance)
- **Pitch:** Hands-free, phone-hosted apps for industrial workers on open glasses platforms, with on-phone Rust perception (barcode/defect cues).
- **Why now:** open SDKs (Meta May 2026, Even Hub Apr 2026, MentraOS MIT); enterprise deployment is Mentra's own pitch.
- **Competitors:** Vuzix/RealWear incumbents; Meta will own the consumer layer.
- **Why labs won't kill it:** distribution is platform-gated and vertical; but *Meta* can.
- **Feasibility:** TypeScript/phone-centric, less Rust leverage; distribution risk high.
- **Scores:** 6 / 7 / 6 / 5 / 9

*Considered and set aside:* a local voice/AI appliance (Home Assistant Voice PE ecosystem). Real demand, but Nabu Casa owns the distribution and the founder can't build hardware; best pursued as a contribution to Idea A's runtime (wake-word/STT on NPU).

## 4. Ranked shortlist

1. **A — Rust real-time VLA/policy edge runtime** (total 42/50): best founder fit, clear unfilled gap, natural OSS wedge into paid support/licensing with OEMs.
2. **C — Cross-NPU Rust deployment layer** (41/50): biggest market, strongest lab-immunity; harder start. Could be the second act of A.
3. **B — Teleop dataset QA/conversion CLI** (38/50): fastest to first revenue from data vendors; risk that Hugging Face absorbs it.
4. **D — CRA compliance kit** (38/50): strongest moat, wrong founder profile without a sales co-founder.
5. **E — Glasses vertical apps** (33/50): platform risk from Meta.

**Verdict:** Physical AI is the one AI sector where a frontier-model update is nearly irrelevant to the hard problems — real-time control loops, silicon fragmentation, calibration, and dirty physical data — which makes it unusually safe for a systems founder. The money, however, is concentrated in a handful of $10B+ model/humanoid companies while software tooling gets ~1.6% of capital, so the right shape is a bootstrapped, revenue-first "picks and shovels" business (runtime, data QA, compliance) rather than a venture-scale bet. Start with the Rust edge-policy runtime on Jetson + SO-101, use the LeRobot/openpi communities as distribution, and let NPU portability and data QA follow from customer pull.
