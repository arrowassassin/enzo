# Deep dive 1 — Human-provenance and AI-disclosure infrastructure

*2026-09-02. 45 searches; every WebFetch was egress-blocked, so all claims are snippet-derived. Prior-evidence items (Deezer, Spotify Verified, Authors Guild seal, Human Made Mark, Steam 30.8%, Art. 50 dates, Pangram $9M, YC RFS, C2PA hardware) are built on, not re-verified.*

## 1. Competitive landscape

**Capture/asset provenance (C2PA layer)**
- **Truepic** — C2PA founding member; Lens SDK + Vision virtual-inspection for insurance/lending. ~$37–39M raised (Series B $26M, M12-led, 2021), investors include Adobe, Sony, Hearst. Bloomberg-estimated ~$15M revenue; customers Security Mutual, Credibly, Microsoft. ([Tracxn](https://tracxn.com/d/companies/truepic/__-LPZGk_J0TfwpmnXVIEdFkFzy-aLYhCFCt8sq9Fq610/funding-and-investors), [checkthat.ai](https://checkthat.ai/brands/truepic), [Truepic blog](https://www.truepic.com/blog/security-mutual-insurance-optimizes-underwriting-with-secure-virtual-inspections)). *Gap:* enterprise capture only; no creator identity, no authorship claims, no compliance audit.
- **Digimarc** — first C2PA 2.1-approved watermark; but Q2'26 revenue $7.4M (−7.8% YoY), $12.4M operating loss, going-concern warning, ~$150M market cap. ([Digimarc PR](https://www.digimarc.com/press-releases/2024/10/08/digimarc-brings-digital-watermarking-c2pa-21-standard), [Quiver](https://www.quiverquant.com/news/Digimarc+Corporation+(DMRC)+Releases+Q2+2026+Earnings:+Revenue+Declines+and+Losses+Widen), [companiesmarketcap](https://companiesmarketcap.com/digimarc/marketcap/)). *Gap:* a cautionary comparable — the durable-mark layer alone has not monetized.
- **Trufo** — cryptographic watermark that survives edits; Orange Logic/National Geographic workflow; IBC accelerator with Sony, castLabs. No disclosed funding. ([Trufo](https://trufo.ai/projects), [IBC](https://show.ibc.org/accelerator-project-stamping-content-c2pa-provenance)).
- **Numbers Protocol (Capture Cam)**, **Nodle Click** (C2PA + blockchain camera app; Vivendi deal), **Attestiv** ($5.6M raised; insurance forensics via ReSource Pro/Duck Creek), **Serelay** (newsroom UGC verification). ([Numbers](https://numbersprotocol.io/), [CAI on Click](https://contentauthenticity.org/blog/community-story-click), [PitchBook Attestiv](https://pitchbook.com/profiles/company/279942-85), [CredCatalog Serelay](https://credibilitycoalition.org/credcatalog/project/serelay/)). *Gap:* all are single-modality, no cross-platform identity, small.
- **Adobe Content Authenticity app** — free, no CC subscription required; CAI at ~6,000 members. ([RedShark](https://www.redsharknews.com/adobes-free-content-authenticity-app-public-beta-released), [CAI 2026](https://contentauthenticity.org/blog/the-state-of-content-authenticity-in-2026)). **Sony Camera Authenticity** — licensed, video added Mar 2026, newsroom-only. ([Sony](https://authenticity.sony.net/camera/en-us/)). *Gap:* signing is free/commoditized; nobody sells verification-as-a-service to platforms.

**Detection (adversarial layer)**
- **GPTZero** — $30M ARR, 19M users, $13.5M raised, acquired by Superhuman (Grammarly) June 2026, terms undisclosed, PitchBook valuation >$88M. ([TechCrunch](https://techcrunch.com/2026/06/23/superhuman-acquires-ai-detection-startup-gptzero/), [Sacra](https://sacra.com/c/gptzero/)).
- **Pangram** — $9M (Menlo, Jul 2026), ~$13M total; 2.7k→120k MAU, revenue 35x YoY; API $0.05/100 words. ([SiliconANGLE](https://siliconangle.com/2026/07/29/pangram-labs-raises-9m-launch-accurate-ai-detection-text-images/), [eesel](https://www.eesel.ai/blog/pangram-4-pricing)).
- **Turnitin** — 16,000 institutions, 185 countries; 14.8% of submissions ≥80% AI. **Clarity** (Mar 2025) records keystrokes/pastes/version history, per-seat add-on, quote-based. ([Turnitin PR](https://www.turnitin.com/press/turnitin-launches-turnitin-clarity-bringing-transparency-and-integrity-insights-to-education), [edusageai](https://www.edusageai.com/blogs/turnitin-pricing-for-teachers-and-schools-in-2026-what-you-can-actually-buy)).
- **Copyleaks** ($13.99–74.99/mo), **Originality.ai** ($14.95–179/mo). ([eyesift](https://www.eyesift.com/blog/ai-detector-pricing-2026/)). *Gap for all:* post-hoc probabilistic; they are the party generating false accusations, not the party clearing humans.

**Process provenance (writing)**
- **Grammarly Authorship** — free; tracks typed vs pasted vs AI-generated in Docs/Word/Blackboard; Rowan-Cabarrus cut integrity violations 27→1. ([Grammarly](https://www.grammarly.com/blog/product/grammarly-authorship-is-now-available-in-blackboard/), [Tech & Learning](https://www.techlearning.com/news/grammarly-authorship-i-tested-the-new-ai-and-plagiarism-tool)). Grammarly now owns GPTZero too — the most dangerous incumbent for the student wedge.
- **ValidDraft** — "prove your writing is human" microstartup. **Thesify Coauthor** — provenance mapping for academic manuscripts. ([Thesify](https://www.thesify.ai/blog/ai-policies-academic-publishing-2026)).

**Human certification marks**
- **Authors Guild Human Authored** — $10/book non-members, free for members, open to all US authors since Mar 2, 2026; no published uptake count. ([Authors Guild](https://authorsguild.org/news/human-authored-certification-expands-to-all-authors/)).
- **Human Made Mark** (film, 0.25% of budget) and **Human Made Inc**. ([LBB](https://lbbonline.com/news/human-made-mark-ai-label-craft-fairtrade-advertising-william-grave), [Variety](https://variety.com/2026/film/global/the-human-made-mark-ai-free-film-initiative-launches-1236728524/)).
- **Credtent** (free three-badge system, human review), **Not By AI** (free badge), **VerifiedHuman**, **AI-Free Cert**, **Fairly Trained** (certifies models, not creators), **Cara** (volunteer-run artist network). ([Credtent](https://medium.com/credtent-on-content/credtent-inc-launches-content-origin-badges-raising-the-bar-for-ai-transparency-5a10cc87dae7), [notbyai](https://notbyai.fyi/), [Cara](https://en.wikipedia.org/wiki/Cara_(app))). *Gap:* all are self-attestation badges with no cryptographic process evidence and no cross-platform recognition — exactly the whitespace, but also evidence that the badge itself is near-free.

**Proof-of-personhood**
- **World ID** ($2.5B valuation, pivoting to enterprise fees), **Humanity Protocol** ($50M raised, $1.1B val, abandoned PoP for "Proof-of-Trust"), **Human.org** ($7.3M pre-seed), **Proof of Human (YC)**, **Reclaim Protocol**. ([BiometricUpdate World](https://www.biometricupdate.com/202606/world-shifts-from-crypto-identity-experiment-to-enterprise-proof-of-humanity), [BiometricUpdate Humanity](https://www.biometricupdate.com/202602/humanity-protocol-pivots-from-proof-of-personhood-but-sticks-with-palm-biometrics), [TechStartups](https://techstartups.com/2025/02/11/human-org-a-startup-building-a-platform-to-prove-youre-human-and-not-ai-raises-7-3m-in-pre-seed-funding/), [YC](https://www.ycombinator.com/companies/proof-of-human)). *Gap:* they prove a human exists, not that a human made this work. Two $1B+ PoP companies have pivoted for lack of revenue.

**Studio/publisher disclosure:** only indie utilities (ravy.pro Steam disclosure generator); Steam's June 2026 per-asset Content Survey tags are self-declared, with indie backlash. ([ravy.pro](https://ravy.pro/tools/steam-ai-disclosure), [remio](https://www.remio.ai/post/indie-game-studios-are-rejecting-steam-s-new-mandatory-ai-metadata-tags)). Scholarly publishing: COPE/STM drafting a "Global Reporting Standard for AI Disclosure." **No funded company owns studio asset provenance** — genuine whitespace.

## 2. Market size (bottom-up)

| Segment | Addressable | Realistic paying | Price | Revenue pool |
|---|---|---|---|---|
| Creators (207M total; ~2M full-time) ([Uscreen](https://www.uscreen.tv/blog/creator-economy-statistics/)) | 2M | 2–5% | $5–10/mo | $12–120M |
| Students/institutions (Turnitin: 16k institutions) | 16k | 500–1,500 | $2–5/seat/yr add-on | $10–75M |
| Game studios disclosing on Steam (7,300+ games) ([Pikorafy](https://pikorafy.com/blog/steam-ai-games-disclosure-surge-2026)) | ~5k studios | 500–1,000 | $1–10k/yr | $2–10M |
| EU deployers of gen-AI facing public (190 CoP signatories incl. Lufthansa, Getty, Iberdrola) ([EC](https://digital-strategy.ec.europa.eu/en/news/strong-backing-code-practice-transparency-ai-generated-content)) | 50–100k firms | 1–3k | $10–50k/yr | $20–150M |
| Insurance photo authenticity (20–30% of claims carry AI-altered media) ([SimpleSolve](https://www.simplesolve.com/blog/ai-altered-media-insurance-claims)) | Truepic/Attestiv already here | — | per-inspection | Truepic ~$15M today |

Existing spend anchors: Turnitin was $100–200M revenue in 2018 (acquired at ~10x) ([EdSurge](https://www.edsurge.com/news/2019-03-06-turnitin-to-be-acquired-by-advance-publications-for-1-75b)); GPTZero $30M ARR; C2PA signing certs ~$289/yr ([SSL.com](https://www.ssl.com/article/eu-ai-act-article-50-a-complete-guide-to-ai-transparency-compliance/)). Total realistic near-term pool: ~$50–350M/yr across all wedges — real but fragmented, mostly EU compliance and education.

## 3. Willingness to pay — evidence

- **Yes, institutions:** Turnitin Clarity is a paid per-seat add-on with multi-university rollouts ([ProofreaderPro](https://proofreaderpro.ai/blog/turnitin-clarity-explained)). GPTZero grew to $30M ARR.
- **Yes, enterprises:** Truepic ~$15M revenue; Attestiv embedded in Duck Creek; SAS survey: 99% of insurers encountered manipulated media ([SAS](https://www.sas.com/en_ca/news/press-releases/2026/may/synthetic-images-ai-insurance-fraud.html)).
- **Yes, industry bodies:** Deezer licensed its detector to Sacem commercially (Jan 2026) and is selling to other DSPs/CMOs ([Rappler](https://www.rappler.com/technology/deezer-artificial-intelligence-music-detection-tool-france-sacem-license/)).
- **Weak, creators:** Authors Guild is $10/book with no published uptake; Adobe, Credtent, Not By AI are free; Spotify Verified is free and already covers "hundreds of thousands" of profiles ([Spotify](https://newsroom.spotify.com/2026-08-11/ai-persona-badges-transparency/)). Consumer-creator WTP is unproven and incumbents set the price at zero.
- **Unknown, studios:** Human Made Mark's 0.25%-of-budget model exists but no signed-production count found.

## 4. Regulatory

- **Art. 50 (EU):** Providers (50(2)) must mark synthetic output in machine-readable, detectable form; deployers (50(4)) must visibly label deepfakes and AI-generated public-interest text unless human editorial control is exercised. Live 2 Aug 2026; systems placed on market before then have until **2 Dec 2026** for 50(2). ([GT Law](https://www.gtlaw.com/en/insights/2026/6/deepfakes-chatbots-ai-generated-text-european-commission-details-transparency-obligations-under-the-ai-act), [EC FAQ](https://digital-strategy.ec.europa.eu/en/faqs/transparency-obligations-under-article-50-ai-act)).
- **Code of Practice:** final version **10 June 2026**; ~190 signatories; requires a multi-layered mark (metadata + watermark at minimum). Voluntary, but compliance presumption. ([WSGR](https://www.wsgr.com/en/insights/eu-commission-publishes-ai-transparency-code-of-practice.html), [Bird & Bird](https://www.twobirds.com/en/insights/2026/taking-the-eu-ai-act-to-practice-the-final-transparency-code-of-practice)).
- **Penalties:** up to €15M or 3% global turnover (Art. 99). ([aiactblog.nl](https://www.aiactblog.nl/en/posts/article-50-enforcement-fines-ai-act-2026)).
- **California SB 942/AB 853:** operative 2 Aug 2026; providers >1M users must offer free detection tool + latent disclosure; $5,000/violation; large-platform duties 1 Jan 2027; capture-device duties 1 Jan 2028. ([Troutman](https://www.troutmanprivacy.com/2025/10/california-ai-transparency-act-amendments-signed-into-law/)).
- **Korea AI Basic Act:** in force 22 Jan 2026; fines up to KRW 30M (~$21k). ([Stimson](https://www.stimson.org/2026/south-koreas-ai-basic-act-seeking-balance-between-industry-innovation-and-social-risk/)).

Crucial nuance: **no regime obliges anyone to certify human authorship.** The law taxes AI output; it does not reward proving humanity. Mark-survivability is a provider (lab) burden under the Code, not a deployer one.

## 5. Technical feasibility — the hard problem

- **Keystroke provenance is broken as proof:** a 2026 arXiv paper shows timing-forgery attacks evade keystroke classifiers at ≥99.8%, and the "copy-type" attack (human retypes ChatGPT output) is *provably non-identifiable* from timing alone. ([arXiv 2601.17280](https://arxiv.org/html/2601.17280)). A follow-on ZK-PoP paper claims lag-1 autocorrelation and revision entropy separate composing from transcribing, plus a TEE-based architecture — promising but unproven. ([arXiv 2603.00179](https://arxiv.org/abs/2603.00179)). Chronicle framed Clarity as surveillance ([Chronicle](https://www.chronicle.com/article/to-catch-ai-cheating-turnitin-wants-to-monitor-students-keystrokes)).
- **Grammarly Authorship** works as a *paste ledger*, not authorship proof. It is free and bundled — hard to out-distribute.
- **C2PA reality:** <1% of news images carry C2PA; WhatsApp, iMessage, Facebook re-encode and strip; YouTube now auto-applies labels from C2PA; Cloudflare/LinkedIn/TikTok preserve. ([SoftwareSeni](https://www.softwareseni.com/c2pa-adoption-in-2026-hardware-platforms-and-verification-reality/), [YouTube Help](https://support.google.com/youtube/answer/15447836?hl=en)). Diffusion-based edits defeat robust watermarks ([arXiv 2603.12949](https://arxiv.org/pdf/2603.12949)). Mark-survivability testing across CMS/CDN/social is therefore a real, measurable service.
- **Registry interop:** Spotify Verified/AI Persona, Apple Transparency Tags, DistroKid AI Credits, and the July 2026 IFPI/RIAA two-label framework are all *platform-owned, self-declared* systems with no external oracle ([Deadline](https://deadline.com/2026/07/music-ai-labels-recording-industry-1236979107/), [RightsDocket](https://www.rightsdocket.com/insights/ai-music-disclosure-distrokid-spotify-apple-music-2026)). Platforms have shown no appetite to outsource.

## 6. Business model options

| Model | Price benchmark | Year-1 revenue? |
|---|---|---|
| Creator subscription | $5–10/mo (vs. free Adobe/Credtent/Spotify) | No — WTP unproven |
| Platform verification API | $0.05/100 words (Pangram), $0.10/verification (Reclaim) | Slow; platforms build in-house |
| Studio disclosure SaaS (pipeline plugin, per-asset ledger) | $1–10k/yr per studio | Yes, small — forced by Steam June 2026 rule |
| EU deployer audit SaaS (disclosure injection + survivability testing + evidence pack) | $10–50k/yr | **Yes** — 2 Dec deadline, 190 CoP signatories, €15M fines |
| Insurance/claims provenance | per-inspection | Occupied by Truepic/Attestiv |

## 7. Team and capital to $1M ARR

Team of 6–8: two applied-crypto/C2PA engineers, one media-pipeline engineer, one EU AI Act lawyer/compliance lead, one game-pipeline engineer, one sales, one founder-CEO. Burn ~$1.6–2.2M/yr. With $10–50k ACVs, $1M ARR is 20–100 EU/studio customers, plausibly 15–20 months post-launch. Estimate **$3–4M seed** to reach $1M ARR.

## 8. Kill risks

1. **Platforms self-supply:** Spotify, YouTube, Meta, Deezer (now *selling* its detector), Steam. Every large platform has chosen in-house self-declaration.
2. **Grammarly/Superhuman** owns Authorship + GPTZero and distributes free inside Word/Docs/Blackboard — the student wedge is already bundled.
3. **Adobe/Google absorb the signing layer** for free; Digimarc shows the durable-mark layer doesn't monetize.
4. **The core primitive is contested:** copy-type attacks make process provenance probabilistic, and false positives from a registry create liability.
5. **Regulation taxes AI, not rewards humans;** no compliance pull for a human registry.
6. **PoP graveyard:** World and Humanity Protocol both pivoted for lack of revenue.
7. **C2PA politics:** trust list gated by DigiCert/SSL.com and Adobe-led governance.

## 9. Comparables

- Turnitin → Advance, $1.75B, ~10x revenue (2019).
- GPTZero → Superhuman, undisclosed, $30M ARR, PitchBook >$88M.
- Clarity (deepfake identity) → Deel, $45–50M, Aug 2026. ([Calcalist](https://www.calcalistech.com/ctechnews/article/bkeyex0sgl))
- Koi → Palo Alto, ~$400M, Apr 2026 (adjacent). ([Calcalist](https://www.calcalistech.com/ctechnews/article/nu6ccmpyw))
- Digimarc: ~$150M market cap, shrinking. Truepic: ~$39M raised, ~$15M revenue, no exit after 11 years.

Pattern: exits are $50–400M strategic tuck-ins; nothing in provenance has exceeded Turnitin's 2019 print.

## Verdict

1. **Not venture-scale as pitched.** The cross-platform verified-human registry faces free incumbents, zero regulatory pull, a contested primitive (copy-type attack), and two $1B+ proof-of-personhood pivots as a warning.
2. **Two halves are a real business:** EU Art. 50 deployer evidence packs + mark-survivability testing (2 Dec 2026 forcing function, €15M fines, 190 CoP signatories) and Steam-driven studio asset ledgers — both B2B, both with year-1 revenue.
3. That combination is a **$5–30M ARR compliance SaaS** with a $50–400M strategic exit — a good lifestyle-plus or small-VC outcome, not a category-defining company.
4. **The registry is a feature** the platforms will keep in-house; pursue it only as a later interoperability layer.
5. **Wedge:** ship an EU deployer "disclosure survivability audit" before 2 Dec 2026; add a Steam/Unreal per-asset provenance plugin in Q1 2027; defer creators and students entirely.
