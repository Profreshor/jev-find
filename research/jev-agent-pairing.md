# Pairing Jev (TypeSafe AI) with coding agents (Claude Code / Codex)

Research date: 2026-09-25. Primary source: live docs at https://docs.typesafe.ai (index: https://docs.typesafe.ai/llms.txt), read via `.md` suffix. Secondary: GitHub (official `typesafe-ai` org + community repos), Vercel/Cloudflare changelogs, Hacker News, one independent practitioner blog.

---

## API facts (verified from docs.typesafe.ai)

**Endpoint / auth**
```
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <TYPESAFE_API_KEY>
Content-Type: application/json
```
`GET https://api.typesafe.ai/v1/models` lists callable model names/aliases.
Source: https://docs.typesafe.ai/api.md, https://docs.typesafe.ai/introduction/quickstart.md

**Request** — one `state` (string, JSON object, or array of text), one `model`, a map of named `questions`. Each question is `type: "choice" | "score" | "noul"` + `instructions` (string/object/array) + type-specific `criteria`. Question IDs are local only, never sent to the model.

**Response** — `model` (the resolved versioned id), `answers` keyed by the same question ids, `usage.{input_tokens,output_tokens}`.

Minimal example (from the quickstart page):
```json
// POST /v1/systemone
{
  "state": "Hi, I've been trying to connect my Stripe account for 3 days and the integration keeps failing. I'm losing sales. Please help ASAP.",
  "model": "jev-latest",
  "questions": {
    "urgency": { "type": "noul", "instructions": "Does this message express urgency?" }
  }
}
```
```json
// 200 response
{
  "model": "jev-1.13.0",
  "answers": {
    "urgency": { "type": "noul", "noul": 0.95 }
  },
  "usage": { "input_tokens": 296, "output_tokens": 20 }
}
```
Choice answers add `choice` + `probabilities` (sum to 1) + `confidence`. Score answers add `score` (can fall between levels) + `legend` + `probabilities` + `confidence`. Noul has no `confidence` field. Source: https://docs.typesafe.ai/api.md

**Batching / limits**
- Many questions per request, evaluated in parallel over the *same* state — this is the core cost/latency lever (see benchmark below). Source: https://docs.typesafe.ai/patterns/fan-out.md
- Choice: max **255 options**. Score: **2–10 ordered levels**. Source: https://docs.typesafe.ai/api.md
- Context: **64k tokens** total per request (state + all questions); **32k tokens** for state + the single longest question. Source: https://docs.typesafe.ai/models.md
- Errors: `401` bad/missing key, `422` validation, `429` rate limit, `529` overloaded. SDKs retry 429/529 with backoff by default. Source: https://docs.typesafe.ai/api.md

**Model / pricing (current: `jev-1.13.0`, alias `jev-latest`)**
- $42/Btok ≈ **$0.042 per 1M input tokens**; output tokens free.
- Rate limits: 250,000 tokens/sec, 1,200 requests/min — documented as adjusting dynamically ("we are serving a very large volume of demand... limits can change without notice"). Source: https://docs.typesafe.ai/models.md
- `jev-preview` currently == `jev-latest` (no preview build live as of doc review).
- Text only (string/JSON/array). No image/audio/video. English strongest; other languages "accepted but currently have lower accuracy." Source: https://docs.typesafe.ai/models.md
- Not fine-tuned per customer; trained via "RLCD" (reinforcement learning for calibrated decisions) for calibrated probabilities, not text quality. Source: https://docs.typesafe.ai/introduction/machine-learning-primer.md

**SDKs**
- Python: `pip install typesafe-sdk` (PyPI), requires Python ≥3.10. `TypeSafeClient` / `AsyncTypeSafeClient`, reads `TYPESAFE_API_KEY` from env. `RetryPolicy` defaults: `max_retries=2`, `backoff_initial=0.5s`, `backoff_max=5.0s`, `backoff_jitter=0.25`, retries on `{408,429,500-599}`, `timeout=30.0s`. Source: https://docs.typesafe.ai/sdk/python.md, https://docs.typesafe.ai/sdk/python/api/retries.md. Source repo: https://github.com/typesafe-ai/typesafe-sdk-python (225★, MIT).
- JS/TS: `npm install @typesafe-ai/sdk`, Node ≥20, ESM+CJS+types. Source: https://docs.typesafe.ai/sdk/javascript.md. Source repo: https://github.com/typesafe-ai/typesafe-sdk-js (237★, MIT).

---

## Official integrations found

1. **TypeSafe agent skill** (the real target for "let Claude Code/Codex call Jev"): https://docs.typesafe.ai/agent-skill.md, code at https://github.com/typesafe-ai/skills (2,122★, MIT, `skills/typesafe-ai/SKILL.md`).
   - Claude Code: `claude plugin marketplace add typesafe-ai/skills` then `claude plugin install typesafe@typesafe-ai`, invoked as `/typesafe:typesafe-ai`.
   - Other agents (explicitly including **Codex**): `npx skills add typesafe-ai/skills --skill typesafe-ai` (project-local by default, `-g` for global).
   - This is a *documentation/context* skill, not a live MCP bridge — it teaches the agent the API shape, primitives, and patterns so the agent **writes code that calls Jev**, the same way it would read any SDK docs. It does not itself expose "call Jev" as a tool the agent invokes mid-turn.
   - Docs explicitly warn: "Coding agents fall into the one question per call habit more than people do" — the skill's job is partly to stop the agent from writing N sequential single-question calls instead of one batched call. Source: https://docs.typesafe.ai/primitives.md
   - `agent-skill.md`'s "Common issues" section is directly useful for building your own tool: keep questions/thresholds in one reviewable file, expect the agent to "invent request or response fields" if the skill/docs are stale, don't over-rely on confidence thresholds when you just want argmax.

2. **"Jev with coding agents" page** — TypeSafe's own explicit positioning: https://docs.typesafe.ai/introduction/coding-agents.md. Direct quote: *"Jev is not a drop-in replacement for the LLM behind Claude Code, Cursor, opencode, Copilot... Instead, you can use your coding agent as usual to write code that uses Jev to make decisions."* This confirms the intended integration shape is "agent writes/calls code against the Jev API," not "Jev sits behind an agent's chat loop." An MCP server that exposes `evaluate(state, questions)` as a callable tool is a reasonable next step beyond what TypeSafe ships today, not something they currently ship.

3. **`typesafe-ai/system-one-adapter-python`** (297★, official org): "Drop-in TypeSafeClient replacement backed by LLM APIs" — i.e. you can swap in OpenAI/Anthropic-backed calls behind the same typed interface for local dev/testing without a Jev key. https://github.com/typesafe-ai/system-one-adapter-python

4. **Vercel AI Gateway** — confirmed via official Vercel changelog (fetched directly): https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway, dated **2026-09-16**. Model id `typesafe-ai/jev`, callable via existing TypeSafe client, the HTTP API, or AI SDK 7's experimental `evaluate` API. Supports Zero Data Retention / No Training. Vercel separately blogged it as "the fastest-adopted model in AI Gateway history" (vendor claim, not independently verified): https://vercel.com/blog/ai-gateway-jev-model-launch

5. **Cloudflare Workers AI** — confirmed via official Cloudflare docs (fetched directly): https://developers.cloudflare.com/ai/models/typesafe/jev/. Model id `typesafe/jev`, listed as third-party, 32k context window, zero data retention. No MCP-specific Cloudflare integration found.

**No official TypeSafe MCP server exists** as of this review. I searched github.com and the docs index for "typesafe mcp," "jev mcp server" and found nothing in the `typesafe-ai` org. Everything MCP-shaped is third-party (next section).

---

## Practitioner patterns

### From TypeSafe's own cookbooks (official, but these are worked examples/benchmarks, not "in the wild" adoption — treat as vendor-authored reference implementations)

- **Skill selection for an agent turn** — directly the "picking which skill/tool applies" use case. Cookbook: https://docs.typesafe.ai/cookbooks/skill_suggestion.md. Setup: an agent harness (Hermes, 182 skills, MIT) truncates skill descriptions to 60 chars in its system prompt, causing wrong/needless skill loads. Fix: two Jev calls per turn — (1) one `Choice` over all 182 skill names (using the same short descriptions) + 3 `Noul` gate questions ("does this need an action / a documented procedure / would prose alone suffice") to decide if a skill is needed at all; (2) re-rank the top-3 with a `Choice` + one `Noul` per candidate against each skill's *full* description + body excerpt (700 chars). Result on 488 test requests against `claude-haiku-4-5`: wrong loads **16.8% → 7.3%** (2.3x fewer), needless loads **9.8% → 4.0%** (2.4x fewer); an oracle (told the right answer) floors at 2.5%/1.2%, so this isn't the ceiling. The suggestion is phrased as advisory ("ignore this if it does not fit") and inserted as a small block *after* the roster so prompt-prefix caching over the roster still holds. It also broke 7 of 315 cases the agent had right on its own — a confident wrong suggestion is more persuasive than none.

- **Triaging line-oriented text ("semantic grep")** — directly the "triage grep/log/test output" use case. Cookbook: https://docs.typesafe.ai/cookbooks/semantic_find.md. Shape: tag every line with an id (`L000`, `L001`...), join into one document, ask one `Choice` question whose 255 options *are the line ids* ("which line answers `<query>`?") plus one `Noul` in the same request checking whether an answer exists at all (Choice probabilities always sum to 1, so something "wins" even with no real match — the Noul is what tells you the ranking is meaningless). Demonstrated on GitHub's 218-line ToS: real answers land `exists ≥ 0.9`+; no-answer cases land `≤ 0.14`. Because Choice caps at 255 options, docs explicitly say to go over that by windowing: rank sections first, then rank lines inside the winning section.

- **Batching cost/speed** — the concrete number behind "put everything in one call." Cookbook: https://docs.typesafe.ai/cookbooks/parallel_questions.md. 13 questions (8 Noul/2 Choice/3 Score) over a ~54k-char document: one batched call vs. 13 single-question calls was **12.2x cheaper and 10.0x faster**, with statistically identical answers (mean/stdev compared over 5 runs each). This is the strongest quantitative case for "don't call the API once per grep line/log entry — batch."

- **Ranking files in a hierarchy** — directly the "reranking files for relevance" use case, and it's literally a codebase example. Cookbook: https://docs.typesafe.ai/cookbooks/hierarchical_classification.md. Uses parallel beam search over `Choice` probabilities to walk a tree to a leaf; one of the four worked hierarchies is a real codebase ("CookSafe files") where both greedy and beam search correctly landed on `retrievers.py`. Beam search (K=3) matched the expected leaf on all 4 test hierarchies (patents, Shopify products, MeSH, code) vs. 2/4 for greedy.

- **Select, don't generate** — directly relevant to pulling values out of diff hunks/log lines without hallucinating them. Cookbook: https://docs.typesafe.ai/cookbooks/pre_parsed_value_extraction_cookbook.md. Pattern: a regex (or any cheap code) finds *candidate* spans (emails, phone numbers, amounts), then one `Choice` question asks Jev to pick which candidate fits a role ("which address should the receipt go to?"); code copies the chosen string verbatim and normalizes it. Jev never re-types the value, only selects an index — eliminates a whole class of extraction hallucination. Two stated limits: 255-option Choice cap (two-stage narrowing above that), and "finding candidates is the hard part" — for unstructured entities like names you still need a regex/NER/LLM proposer upstream.

- **Mapping NL to typed function calls** — directly the "picking which tool/skill applies, with arguments" use case. Cookbook: https://docs.typesafe.ai/cookbooks/function_calling.md. Every `Literal`-typed function argument becomes a `Choice`; a companion `Noul` per argument ("does the user even say anything about this?") lets the call omit the argument and fall back to the function's default rather than forcing a guess. `confidence` on the overall call is reported as the *weakest* argument's confidence, not a product — the docs explicitly reject multiplying confidences because it falsely penalizes calls with more arguments.

- **Question wording changes results more than you'd expect** — a concrete gotcha from the structure-recovery cookbook (https://docs.typesafe.ai/cookbooks/autoformat.md): asking "are these two lines part of the same paragraph?" caused every unmarked list item to merge into one block (17 blocks → 12, wrongly). Rewording to the literal fact needed ("does this line pick up mid-sentence?") fixed it. Lesson for building a triage tool: word the question as the narrowest fact the threshold actually needs, not the closest natural-language paraphrase.

### Independent / community (verified where noted; treat freshness as a caveat — most of this is under 10 days old relative to this review)

- **Flavio Copes** (individual, known JS/web dev blogger — not affiliated with TypeSafe), https://flaviocopes.com/jev/ — hands-on but explicitly pre-production ("I have console access but haven't put Jev into production yet"), recommends shadow-mode (run Jev beside existing logic, log full answers, don't switch over blind). Concrete agent-pairing use cases he names:
  - **Shell command safety gate**: classify a command as read-only / reversible / irreversible before a coding agent executes it.
  - **Browser automation**: an LLM plans the goal, Jev picks which element to click from an accessibility-tree snapshot each step.
  - **Code review triage**: rank changed files by risk, send only the top-ranked chunks to a (more expensive) coding model for explanation/fixes — this is close to "rerank files for relevance to a task."

- **`jgrep`** — the closest thing to a standard tool for "triage grep/test output before it hits agent context." Multiple *independent* implementations appeared within days of each other, all doing essentially the same thing: chunk a file into 5–60 line blocks, one `Noul` per chunk asking "does this match `<English description>`?", batched ~16 chunks/request, grep-style `file:line` output with CI-friendly exit codes, an opt-in `--diff` mode over git hunks, and (in at least one) an opt-in Claude Code/Codex skill.
  - https://github.com/kyu1204/jgrep (TypeScript, npm package `jevgrep`, 34★, real 230KB codebase, created 2026-09-19, still being pushed to as of 2026-09-24) — verified via GitHub API, not just README claims.
  - https://github.com/keltokhy/jgrep (Python variant, same pitch)
  - https://github.com/Neel49/jgrep, https://github.com/Emasoft/jgrep (further independent reimplementations)
  - None of these are affiliated with TypeSafe. No consolidation into one canonical tool yet — four+ competing implementations of the same idea in one week is a strong signal the *use case* is real and obvious, but no single tool has traction to point to.

- **MCP servers** — several independent, all created 2026-09-17 to 2026-09-23 (i.e. in the week before this review), all low-star, unconsolidated:
  - https://github.com/rashedInt32/jev-mcp — "classify, score, check, batched ask," ships as a Claude Code plugin (8★).
  - https://github.com/BYK/jev-mcp — "eval-first," verified via GitHub API to have a real src/test/examples layout, not a stub (3★, created 2026-09-17).
  - https://github.com/darthzen/jev-mcp — single `evaluate` tool over streamable HTTP.
  - https://github.com/legostin/jev-mcp — narrower: browser automation specifically (drives Chrome via Jev decisions).
  - https://github.com/itsmostafa/typesafe-mcp, https://github.com/FrancoisChastel/jev-code, https://github.com/shitianfang/jev-use — same pitch with variations ("plus open-weight model support," "one-command setup for Claude Code/Codex/Pi/OpenCode," "measured p50 ~230ms and ~$0.02/1,000 judgments").
  - **Take-away**: the MCP-server idea this task is scoping is already being built by multiple independent people right now, at very early/unvetted maturity (single-digit stars, days old, no apparent code review or adoption signal beyond the README). None of them is an obvious "adopt this" choice yet — worth building your own rather than depending on one of these.

- **Hacker News** — real, verified thread (fetched via Algolia API, not trusted from search-summary text — see caveat below): story "OpenJev" (an apparent open-weights alternative/clone), https://news.ycombinator.com/item?id=49752041, 721 points, by user `ilreb`, 2026-09-18. Genuine technical skepticism in the comments, e.g. one subthread (https://news.ycombinator.com/item?id=49754313 and replies) arguing over whether Jev is architecturally different from an LLM at all or "just" a classifier/logprobs wrapper around a fine-tuned model — TypeSafe's own claim that it's not generating text internally is taken on faith by commenters, not verified, since it's a closed model.
  - Related, unverified-in-depth: web search summarized r/LocalLLaMA reactions comparing Jev to existing zero-shot classifier encoders (e.g. GLiNER-style models) and questioning novelty. I was not able to independently open/confirm the specific Reddit thread, so treat this as secondhand.

---

## Candidate use cases for a coding agent, ranked by value

1. **Triage large tool output (grep/log/test/diff) before it reaches the agent's context.** Best-evidenced item on this list: official `semantic_find` cookbook pattern (tag lines, one `Choice` over line-ids + one `Noul` for "is there even an answer") plus four independent community `jgrep` implementations converging on the same chunk-and-batch design. Question shape: state = the tool output split into ids (files, lines, or diff hunks, 5–60 "lines" per chunk to stay well under the 255-option Choice cap and the 32k single-question budget); one `Noul` per chunk ("does this match `<task-relevant description>`?") or one `Choice` over chunk-ids for "which chunk is most relevant to X," always batched, never one call per line.

2. **Pick which skill/tool/subagent applies to the current turn.** Second-best evidenced: official `skill_suggestion` cookbook, with a real before/after benchmark (2.3–2.4x fewer wrong/needless loads) on an agent harness structurally similar to Claude Code's skill system. Question shape: one wide `Choice` over all available skill/tool names using their existing short descriptions (cheap, prefix-cacheable) + a couple of gate `Noul`s ("does this need an action / documented steps, or does prose suffice"), then — only if something scored above threshold — a second narrower `Choice` + per-candidate `Noul`s over the top 2–3 candidates' full descriptions.

3. **Rerank files/context for relevance to a task.** Evidenced by the `hierarchical_classification` cookbook (codebase file-tree beam search, correctly reaching `retrievers.py`) and independently named by Flavio Copes ("rank changed files by risk, send top chunks to a coding model"). Question shape: `Choice` over candidate file paths (or a beam-search walk down a directory tree if the candidate set is large), criteria = short per-file purpose strings your code already has (docstring, last-commit message, etc.), instructions = the task description.

4. **Classify diff hunks / verify a change against a stated intent.** Evidenced by the `citation_check` cookbook shape (check a claim against its source — same shape as "does this hunk actually do what the commit message says") and Copes's shell-command-safety-gate pattern. Question shape: one `Noul` per hunk against the stated task/intent, or a `Score` (e.g. risk 0–3) if you need to rank rather than gate; batch all hunks in a diff into one request.

5. **Fill typed tool/function arguments from a natural-language request.** Well-evidenced by the official `function_calling` cookbook — every closed-set argument becomes a `Choice`, with a paired `Noul` for "was this even specified" so unspecified args fall through to code defaults instead of being guessed. Directly reusable for turning a free-text agent instruction into a validated call to one of your own typed tools.

6. **Dedupe near-duplicate issues/records.** Not directly read in depth (only its one-line description from the docs index), but the `entity_alignment` cookbook is explicitly this shape — one `Score` plus companion `Noul`s per field, deciding whether two catalogue records describe the same entity. https://docs.typesafe.ai/cookbooks/entity_alignment.md — worth a follow-up read before building, since I didn't verify its worked numbers.

7. **In-loop progress/stop checks for an autonomous agent.** Thinnest evidence: one secondhand mention of "an experimental Codex/OpenCode supervisor sends bounded job, output, and diff context to Jev for progress checks," found only via a search-engine summary, not an artifact I opened and verified myself. Treat as an idea, not a validated pattern.

---

## Open questions / limits

From TypeSafe's own "jaggedness" page for the current model (https://docs.typesafe.ai/model-jaggedness/jev-1.13.md, last reviewed 2026-09-17) — all directly relevant to a triage/rerank tool:

- **Literal reading.** Jev answers the exact words in `instructions`, not implied intent. Scoping/negation must be spelled out; ambiguity should be split into two literal questions and combined in code, not left for the model to infer.
- **No reliable counting or arithmetic.** Don't ask "how many of these match" — ask one `Noul` per candidate and sum in code. Don't do date/time math in-model; extract components (month/day/year as closed sets) and compare in code.
- **Numeric/low-level representations underperform semantic ones.** Hex colors, raw coordinates, assembly-level text score worse than named/English equivalents — convert in code first.
- **Indirection costs accuracy.** Multi-hop or "property of a property" questions are unreliable; point directly at the relevant state field.
- **Large state full of irrelevant detail degrades accuracy** ("context rot" — their term) — filter in code before sending, or add an explicit relevance `Noul` if you can't pre-filter.
- **Adversarial/injected content is not treated as hostile by default** — a real concern if tool output being triaged (log lines, file contents, PR descriptions) is attacker-influenced; test against injection before trusting a gate built on it.
- **No cross-question structural guarantees.** A `Noul` on "is this X" and a `Choice` between "X"/"not X" on the same input are *not* directly comparable (docs show a 0.22 Noul vs. 0.01/0.99 Choice on the same ticket); `P(noul)` and `1 - P(not-noul)` on separately-asked negated Nouls don't sum to 1 either (0.72 + 0.47 = 1.19 in their own example). Don't port a threshold tuned on one question shape to another, and don't expect complementary questions to be arithmetic inverses of each other.
- **No text generation** — cannot produce prose/code/explanations, only constrained typed values; a triage tool needs a separate step (or a generative model) if it needs to explain *why* something was flagged.

Other gaps from this review, not covered by TypeSafe's own docs:
- **No official MCP server.** Everything MCP-shaped is third-party, <10 days old, single-digit-star, unconsolidated (see practitioner section). Building one is genuinely unclaimed territory, not a "why reinvent this" situation.
- **Rate limits are explicitly unstable right now** ("adjusting dynamically... can change without notice") per https://docs.typesafe.ai/models.md — a production triage tool that fires many batched requests should build in 429/529 backoff (SDK default handles this) and not hard-code the documented 250k tok/s, 1,200 req/min figures as guarantees.
- **Vendor performance claims are unverified by a third party.** "193.6x faster and 444.6x cheaper than LLMs" and "70–500ms end-to-end" are TypeSafe's own reported numbers (via the Vercel changelog and TypeSafe's launch materials), not something I found independently benchmarked. The cookbooks' own numbers (12.2x/10.0x batching; 2.3x/2.4x skill-load error reduction; 5%→18%/38%→62% rerank accuracy) are more trustworthy since they show methodology, but are still TypeSafe-authored worked examples, not third-party evals.
- **Caution on search-tool output itself:** one WebSearch-tool summary in this session asserted the HN "OpenJev" thread had "1,821 points and 480 comments" — verified directly against the HN Algolia API to actually be 721 points with a top-level child count of 58. That specific claim was fabricated by the summarizer, not by any source page. Anything in this file drawn from a direct fetch (docs.typesafe.ai, GitHub API, Vercel/Cloudflare changelog, HN Algolia API) is verified; anything attributed only to a search-summary blurb is flagged as such above.
- **Heavy SEO/content-farm noise around "Jev."** A large cluster of cross-linking "awesome-jev" GitHub lists (Anil-matcha, yibie, cobanov, AnotiaWang, AbdelStark, valentynkit, MrJev, Jessie-QingYu) and dozens of near-identical "What is Jev" explainer blog posts (firecrawl.dev, kdnuggets, mindstudio.ai, daily.dev, explainx.ai, dev.to, redhub.ai, apimaster.ai, jevlist.ai, jevmodel.org, awesomejev.com) appeared within days of launch. One of these repos (`Anil-matcha/awesome-jev-by-typesafe`) shows a GitHub `created_at` of 2023-05-17 — three years before Jev existed — strongly suggesting a repurposed/recycled repo, a known content-farm pattern. None of this was treated as a source above; flagging it so it isn't mistaken for organic adoption signal if re-searched later.
