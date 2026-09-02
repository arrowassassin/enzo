# Deep dive 3 — Eldercare "family chief of staff" agent

*2026-09-02. 45 searches; WebFetch reached only one page in full (the Fasten GitHub README); everything else is from search-result snippets. The 53M caregiver figure used earlier is stale: AARP/NAC's Caregiving in the US 2025 puts it at **63M** ([AARP](https://www.aarp.org/press/releases/2025-07-24-new-report-reveals-crisis-point-for-americas-63-million-family-caregivers.html)).*

## 1. Competitive landscape

Nobody funded is doing the full bundle (delegated authority + outbound voice + inbound scam screen + sibling thread + human desk). But every piece has an incumbent, and two of the most valuable wedges are already being given away.

| Company | What | Funding / status | Price | Gap vs. the idea |
|---|---|---|---|---|
| **Carefull** | Transaction monitoring, credit/title, trusted-contact and family-coordination tools; "GreyMatter" AI layer | ~$19.7M total (Fin Capital, Bessemer). Nov 2025: approved for Osaic's 11K advisors. **Jun 2026: Edward Jones gives it free to ~9M clients** ([PRN](https://www.prnewswire.com/news-releases/edward-jones-introduces-carefull-financial-safety-platform-to-help-families-protect-what-matters-most-302808126.html)) | $9.99/mo annual | Read-only monitoring; no action-taking, no voice. Consumer fraud-monitoring is now a free feature. |
| **EverSafe** | Account/credit/identity monitoring with alerts to a family "circle" | Bootstrapped; ABA-endorsed | $7.49 / $14.99 / $24.99/mo ([AARP](https://www.aarp.org/personal-technology/tools-to-avoid-elder-financial-abuse/)) | Monitoring only. Anchors WTP for fraud-watch at ~$8–25/mo. |
| **SilverBills** | Humans receive, review, pay seniors' bills; insured | $1.9M (NIH SBIR + angels), operating since 2014 ([NIA](https://www.nia.nih.gov/research/sbir/nia-small-business-showcase/silverbills)) | $50–99/mo | Closest to "agent with authority" but human-run, small after 12 years = a warning about consumer economics for admin services. |
| **Solace Health** | Human patient advocates (RNs) who schedule, appeal, coordinate | $130M Series C at **$1B** (Feb 2026, IVP) ([MobiHealthNews](https://www.mobihealthnews.com/news/solace-health-raises-130m-bringing-it-unicorn-status)) | Free to patient: billed to Medicare under 2024 navigation codes ([Solace](https://www.solace.health/articles/how-solace-advocacy-is-covered-by-medicare)) | Proves the "chief of staff for a Medicare patient" job is venture-scale **when Medicare pays**. Human-delivered; not scam/finance/mail. Most dangerous adjacent player. |
| **DUOS** | AI-informed navigation for MA members | $130M growth equity ([Fierce](https://www.fiercehealthcare.com/finance/duos-snags-130m-equity-investment-scale-ai-platform-seniors)) | Plan-paid | Plan-side, not family-side. |
| **Wellthy** | Employer benefit; care concierges | ~$76–80M raised; acquired Patch Caregiving Oct 2025 | PMPM, or voluntary at **$200–400/mo** ([LTCI Partners](https://www.ltcipartners.com/group-ltc/3-great-resources-to-help-tackle-the-issue-of-caregiving-at-the-workplace)) | Human-heavy; employer channel. Voluntary price is the ceiling for a human "chief of staff." |
| **Cariloop** | Employer caregiving benefit | $42M; **absorbed Grayce's clients Nov 2025** ([GlobeNewswire](https://www.globenewswire.com/news-release/2025/11/13/3187552/0/en/grayce-embarks-on-a-new-chapter-partners-with-cariloop-to-support-employers-and-their-working-caregivers.html)) | PEPM ~$3–5 | Employer channel consolidating, not expanding. |
| **ianacare** | Free family-coordination app, B2B2C | $16.7M | Free | Coordination is a free feature. |
| **Sensi.ai** | Audio monitoring for home-care agencies | $45M C (Oct 2025), ~$98M total | Agency-paid | Agency side. |
| **Papa / Honor** | Companion visits / home-care ops | Papa $1.4B (2021); Honor $1.25B (2021) | — | No new rounds since 2021; stale marks. |
| **Callie Care** | Phone-first AI that calls seniors daily | $500K pre-seed; 8,000 users tested, 85% continuation ([Menlo Times](https://www.menlotimes.com/post/callie-care-raises-500k-pre-seed-to-tackle-america-s-senior-care-gap-with-phone-first-ai)) | — | Inbound to the senior, not outbound on their behalf. |
| **Mio / Assindo / KallyAI** | Generic AI phone agents; navigate IVR, wait on hold | Unfunded. Mio "never speaks on anyone's behalf" ([Mio](https://mio.gg/guides/phone-calls-for-elderly-parents)); Assindo $70/mo | $5 credit / $70/mo | Commodity horizontal layer; no authority, no eldercare workflow. |
| **Hiya AI Phone** | Screens live and deepfake scam calls in real time | Launched Jan 2025 | $10/mo ([Cybernews](https://cybernews.com/tech/hiya-deepfake-detection-app/)) | Scam-screen wedge already priced at $10. |
| **Google Gemini** | Calls local businesses on your behalf (Duplex), self-identifies as AI; US-wide summer 2026 **except IN, LA, MN, MT, NE** ([Pocket-lint](https://www.pocket-lint.com/google-ai-powered-phone-calls/)) | Free | Horizontal calling is being absorbed; the five excluded states hint at state-law friction. |

YC 2026 pages show adjacent companies (Cova, AI-native home-care agency; Shasta Health, AI calls for clinics) but no family-side authority agent.

## 2. Market size (bottom-up)

- **Population:** 63M caregivers, avg **27 hrs/wk**, ~1 in 4 at 40+ hrs; avg **~$7,200/yr out-of-pocket** ([AARP](https://www.aarp.org/caregiving/basics/caregiving-in-us-survey-2025/)). Carefull's own TAM claim: **45M** adults managing a loved one's finances ([citybiz](https://www.citybiz.co/article/16830/carefull-raises-3-2m/)).
- **Replacement spend:** geriatric care managers $100–200/hr, 100% private pay ([Senior Move Guide](https://www.seniormoveguide.com/geriatric-care-manager-cost-and-services/)); daily money managers $60–80/hr ([AADMM](https://secure.aadmm.com/faqs/)); SilverBills $50–99/mo; Wellthy voluntary $200–400/mo.
- **Employer:** only **11%** of employers offer eldercare services (SHRM 2026, up from 7%); PEPM $3–5 ([Compt](https://compt.io/blog/unraveling-elder-care-employee-benefits-the-ultimate-guide/)).
- **Medicare Advantage:** 2026 rebates of **$2,664/enrollee** fund supplemental benefits; **16% of individual MA plans now offer caregiver support (5% in 2025)** ([KFF](https://www.kff.org/medicare/medicare-advantage-2026-spotlight-a-first-look-at-plan-premiums-and-benefits/), [ATI](https://atiadvisory.com/resources/cy2026-medicare-advantage-trends-supplemental-benefits/)).
- **Direct-pay arithmetic (estimate):** ~40M adult-care caregivers → ~12M in households able to pay $40+/mo → 2–4% conversion = 250–500K families × $480/yr = **$120–240M ARR** at maturity for a pure consumer play. Truebill was acquired at $1.275B with 2.5M members and ~$100M revenue ([TechCrunch](https://techcrunch.com/2021/12/20/rocket-companies-buys-truebill-for-1-275b/)) — the ceiling shape for consumer subscriptions here.

## 3. Willingness to pay

Evidence is priced, not surveyed. Observed price points: monitoring $7–25/mo (EverSafe, Carefull); scam-screen $10/mo (Hiya); generic AI caller $70/mo (Assindo); human bill-pay $50–99/mo (SilverBills); human concierge $200–400/mo (Wellthy voluntary); care manager $100–200/hr. The gap between $25 (software) and $200 (human) is exactly where an agent-plus-desk at **$49–99/mo** would sit — but SilverBills has lived in that gap for 12 years on $1.9M, which says the segment converts slowly without a channel. Caregivers already spend $7,200/yr OOP, so budget exists; the problem is trigger and trust, not price.

## 4. Regulatory and legal

- **POA:** No statute or case law found on delegating agent authority to software; UPOAA imposes personal duties on the agent (good faith, scope, record-keeping) ([UPOAA](https://www.sos.ms.gov/content/documents/pol_res/power%20of%20attorney/8UniformPowerOfAttorneyAct.pdf)). The defensible framing is **"software acting as the human agent's instrument, under the agent's supervision"** — never "AI holds POA." The record-keeping duty is a product feature (audit log).
- **Medicare:** CMS-10106 lets the beneficiary authorize 1-800-MEDICARE to share PHI with a named party; revocable ([CMS](https://www.cms.gov/medicare/cms-forms/cms-forms/downloads/cms10106.pdf)). Insurers accept POA/authorized reps but often demand their own forms — onboarding = a forms campaign per payer/pharmacy.
- **AI outbound calls:** FCC (Feb 2024) treats any AI-generated voice as "artificial" under TCPA ([FCC](https://www.fcc.gov/document/fcc-makes-ai-generated-voices-robocalls-illegal)). B2B calls to insurers/pharmacies are lower-risk; FTC Section 5 and California's BOT Act push for up-front AI disclosure. Infinitus's 4M+ payer calls say payer reps will keep talking to a disclosed bot.
- **Inbound screening:** 11 all-party-consent states ([Rev](https://www.rev.com/blog/phone-call-recording-laws-state)). Screening before connection sidesteps most of it; analyzing the parent's live connected calls does not.
- **Banks:** Senior Safe Act gives banks safe harbor to report exploitation; agencies revised the interagency statement June 2026 ([FDIC](https://www.fdic.gov/news/financial-institution-letters/2024/agencies-issue-interagency-statement-elder-financial)) — tailwind for a bank-partnered product, which Carefull is already riding.

## 5. Technical feasibility

- **IVR/hold voice:** Solved at enterprise grade — Infinitus ($15M A Jun 2025; 4M+ payer calls; bypasses IVR and hold) ([MobiHealthNews](https://www.mobihealthnews.com/news/infinitus-systems-raises-515m-automating-healthcare-calls)). Commodity stacks: Retell/Vapi **$0.11–0.30/min all-in** ([Medium](https://medium.com/@automation.labs/vapi-vs-retell-vs-bland-in-2026-the-true-cost-per-minute-578f38af3523)). A 45-min insurer call costs $5–14; fine at $49+/mo if calls are a few per month.
- **Voice-clone detection:** Pindrop claims 99.2% accuracy; Hiya >97% and says **1 in 4 calls it reviews contain AI audio** ([Hiya](https://blog.hiya.com/hiya-delivers-top-tier-speech-deepfake-detection-on-the-hugging-face-arena)). Licensable, not proprietary.
- **Records:** Fasten Connect (managed API, 50,000+ orgs; on-prem GPL, supports caregiver roles) **[full page]** ([GitHub](https://github.com/fastenhealth/fasten-onprem)); Flexpa payer claims API.
- **Mail:** Earth Class Mail $29–79/mo; PostScan $10–30/mo. Requires redirecting a parent's mail — a large trust step.
- **WhatsApp:** Meta signs no BAA; **US marketing templates paused since Apr 2025** ([Message Central](https://www.messagecentral.com/blog/whatsapp-business-api-usa-complete-guide)). PHI in the thread is out. Ship a native app + SMS/voice; WhatsApp is not viable as the US system of record.

## 6. Distribution

- **Wealth/bank channel is being locked up:** Edward Jones (9M clients), Osaic → Carefull; Raymond James → Cariloop. A newcomer must offer action, not alerts.
- **Employer:** PEPM $3–5, 11% penetration, consolidating. Slow.
- **Medicare/MA:** Solace shows the fastest route — get paid by Medicare per member for navigation; DUOS shows plans will buy AI navigation. 16% of MA plans now offer caregiver support.
- **Payer/user mismatch:** the adult child pays, but the parent must sign POA/CMS-10106, accept call screening, and redirect mail. Callie Care's 85% continuation suggests seniors accept a phone-first agent; the harder step is authority. Amazon's Alexa Together ($19.99/mo, 2021) is the cautionary precedent ([TechCrunch](https://techcrunch.com/2021/12/07/amazon-launches-its-19-99-per-month-alexa-together-elder-care-subscription-for-families/embed/)); it was later discontinued.

## 7. Business model options

1. **Family subscription:** $49/mo core / $99 "Chief of Staff" (unlimited calls, human desk, scam screening). Mature ARR $120–240M.
2. **Employer benefit:** $4–6 PEPM; 3-year sales cycles.
3. **MA/Medicare-billed navigation:** Solace's path. Requires clinical staff and billing infrastructure; converts the desk from cost center to revenue.
4. **Bank/wealth white-label:** need the *action* layer (freeze, dispute, call the bank).
5. **Contingency on recoveries/overbilling:** lumpy; monetizes ~10% of users.

## 8. Team and capital to $1M ARR

Founders: healthcare-ops/voice-AI engineer, elder-law/compliance lead, and someone who has run a care desk (Solace/Wellthy alumni). Plus 2–3 engineers, 2 desk staff (RN/social worker + a bill specialist), elder-law counsel on retainer. Insurance: E&O, cyber, fidelity bond. ~$1.5–2.5M seed for 18 months to 1,000–1,700 families at $49–99/mo.

## 9. Kill risks

- **Absorption:** Google already calls businesses on your behalf in 45 states; Hiya screens deepfakes for $10. Only the *authority + regulated multi-party* layer remains defensible, and it's the hardest to build.
- **Carefull/Solace adding agents:** either can bolt on outbound voice.
- **Liability incident:** one wrong payment or missed appeal under "POA" framing triggers state exploitation statutes.
- **Trust barrier:** parents must hand authority to software owned by one child; sibling disputes are a lawsuit vector.
- **Platform:** WhatsApp unusable for PHI in the US.

## 10. Comparables

Solace $1B (Feb 2026); DUOS $130M growth; Papa $1.4B and Honor $1.25B (2021, stale); Wellthy ~$80M raised; Carefull ~$20M → Edward Jones tie-up; Kinto → Rippl tuck-in; Truebill → Rocket $1.275B at ~$100M revenue, 2.5M members.

## Verdict

1. **Venture-scale only via reimbursement or a plan/payer contract; as a pure family subscription it is a $100–250M-ARR lifestyle-to-mid business with SilverBills-like conversion friction.**
2. The consumer fraud-monitoring and scam-screening wedges are already free (Edward Jones/Carefull) or $10 (Hiya) — do not start there.
3. The defensible core is the **authority layer**: a human-supervised agent that holds forms (POA, CMS-10106, payer/pharmacy reps), keeps the audit log, and does outbound calls — no one on the family side does this.
4. **Start with the "call Medicare/insurers/pharmacies for my parent" wedge**, priced at $79–99/mo with a human desk, framed as "your instrument as POA agent," and instrument it for Medicare navigation billing from day one.
5. Raise on the Solace-for-families thesis, not the ChatGPT-can't-do-this thesis; Google's 45-state calling rollout shows horizontal calling gets absorbed, while regulated authority does not.
