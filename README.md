# jev-find

`jev find` is grep for when you don't know the words. You describe what the code (or log line, or diff hunk) does in plain English, and it scores every chunk of your repo against that description in about a second.

```
$ jev find "implementation code, not tests, that walks up parent directories looking for a justfile" src
src/search.rs:233-269  0.96  }
src/search.rs:164-192  0.92  /// Find justfile starting from parent directory of current justfile
src/subcommand.rs:172-203  0.91  fn run<'src>(
jev: 5 of 843 chunks >= 0.5; next best 0.86 at src/subcommand.rs:204-238 | 843 chunks, 148 files, 253957 tokens, 0.7s
```

I built it for coding agents (Claude Code, Codex), not for people. Agents are good at reasoning and bad at spending their context window on 5,000 lines of grep output. This tool hands the tedious "which of these 2,000 chunks is relevant?" sorting to [TypeSafe's Jev](https://docs.typesafe.ai), a small, fast model that answers yes/no questions with calibrated probabilities. Only the answer reaches the agent's context.

It does one thing. There's no MCP server, no indexing step, no embeddings database, no config file.

## What it's good at

**Finding code by behavior in an unfamiliar repo.** You know what you want ("retries failed requests with backoff") but not what this codebase calls it (`backoff`? `throttle`? `RetryPolicy`?). Agents usually handle this by guessing grep patterns or starting a search subagent that takes 30+ seconds.

**Checking a diff for scope creep.** Each hunk is scored on its own and labelled with its file:

```
$ git diff | jev find "a change that does not relate to increasing the line buffer size"
--- stdin:crates/ignore/src/walk.rs lines 13-25  0.97
...
-            max_filesize: None,
+            max_filesize: Some(10),
jev: 2 of 3 chunks >= 0.5; next best 0.02 at stdin:crates/searcher/src/line_buffer.rs:26-38 | ...
```

**Long command output where grep can't find the cause.** `cargo test 2>&1 | jev find "explains why a test failed"`. Piped input is printed back as text, since the agent can't reread it later.

**Big structured files with cryptic names.** Flatten to one item per line with `jq`, then score each line on its own:

```
jq -r '.components.schemas|to_entries[]|"\(.key): \((.value.properties//{})|keys|join(", "))"' openapi.json \
  | jev find "lease or rental tables" --chunk 1 --top 100
```

## What it doesn't do

- **It doesn't replace grep.** If you know the identifier, grep is exact, instant and free.
- **It doesn't explain anything.** Jev returns probabilities, not prose. You get locations and scores, then you (or your agent) read the code.
- **It doesn't count or do arithmetic.** "How many handlers touch the DB" is not a question it can answer.
- **It isn't deterministic.** A vague query matched 76, 82 and 81 chunks on three identical runs. The top results stay stable. Chunks near the threshold can flip.
- **It reads your description literally.** "Database table for leasing" skipped every `...Request` object, because those aren't tables. Reword and rerun. A run costs about a cent.
- **It doesn't treat file contents as hostile.** Text in the searched files could in principle steer the scores. The worst case for a search tool is hiding or surfacing a chunk, but don't build a security gate on it.
- **It sends your code to TypeSafe's API.** Read their [data policy](https://docs.typesafe.ai/legal) before pointing it at anything you can't share.

## Install

You need a Rust toolchain and a [TypeSafe API key](https://docs.typesafe.ai).

```sh
cargo install --git https://github.com/Profreshor/jev-find

mkdir -p ~/.config/jev
echo 'TYPESAFE_API_KEY=your-key-here' > ~/.config/jev/.env
chmod 600 ~/.config/jev/.env
```

`jev` looks for the key in the environment, then `./.env`, then `~/.config/jev/.env`. The last one exists because Claude Code and Codex don't load shell profiles the same way, and a key exported in `.zshrc` isn't always visible to an agent's shell.

### Hook it up to Claude Code and Codex

The skill in `skills/jev/SKILL.md` tells agents when to use the tool and how to phrase queries. Codex reads skills from `~/.agents/skills` and Claude Code from `~/.claude/skills`, so keep one copy and link it into both:

```sh
git clone https://github.com/Profreshor/jev-find ~/src/jev-find
ln -s ~/src/jev-find/skills/jev ~/.agents/skills/jev
ln -s ../../.agents/skills/jev ~/.claude/skills/jev
```

Skills are optional by design: the agent reads the description and decides whether to load it. For a stronger nudge, add a line to `~/.claude/CLAUDE.md` and `~/.codex/AGENTS.md`, which load in every session:

```markdown
`jev find "<what the code/text does>" [paths]` searches by meaning in about a second (see the `jev` skill). In unfamiliar code, when you know the behavior but not the identifier names, run it before guessing grep patterns or spawning a search subagent. Before reporting a multi-file change done, run `git diff | jev find "a change unrelated to <task>"` on your own change (`git diff <base>` if already committed), and say in your report whether each flagged hunk was intended. Don't redirect its stderr: the summary line is the useful part.
```

## Usage

```
jev find <QUERY> [PATHS]...

  --chunk <LINES>      max lines per chunk (default 40)
  --threshold <P>      minimum probability to count as a match (default 0.5)
  --top <N>            max results to print (default 20)
  --files              one line per file (its best chunk), i.e. rank files by relevance
  --json               machine-readable output
```

With no paths it reads piped stdin, or searches the current directory if nothing is piped. An empty pipe is an error (exit `2`), not a fallback to the current directory. See the changelog for why. It respects `.gitignore`. While walking directories it skips binary files and files over 1 MB, and says how many it skipped. A file you name directly is always searched.

Exit codes work like grep: `0` matches found, `1` no matches, `2` error or nothing to search.

**Read the summary line.** It goes to stderr and always reports the best score that didn't make the cut. "0 matches, next best 0.45" means go look there. "Next best 0.03" is real evidence the thing isn't in those paths. A tool that returns nothing with no context invites false conclusions, and this line is there to prevent that.

## How it works

1. Split each file into chunks of up to 40 lines, preferring to cut at blank lines so functions stay whole. Diffs are split at every hunk and file header.
2. Pack chunks into windows of about 24k characters. Each window becomes one API request: the chunks go in `state`, with one yes/no ([Noul](https://docs.typesafe.ai/primitives/noul)) question per chunk, such as ``Does `chunks.c12` contain content that matches this description: "..."?``
3. Send up to 16 requests in parallel, retrying on 429/529 with backoff.
4. Sort by probability and print.

I tested the alternative of sending one request per chunk. Packing chunks into shared windows was 40% cheaper, a bit faster, and gave fewer false positives. On one unrelated chunk the score dropped from 0.43 to 0.06.

It's about 320 lines of Rust in one file (`src/main.rs`), tests included.

## Test results

These are small tests, enough to show the tool is worth using and not enough for real accuracy numbers. Details in [`research/eval.md`](research/eval.md).

### "Where is X?" on casey/just, against an Explore subagent

10 questions about [casey/just](https://github.com/casey/just) (`src/`: 148 files, ~28k lines), phrased by behavior with no identifiers. For example: "detects circular dependencies between recipes" or "handles the user pressing Ctrl-C while a recipe is running".

| | `jev find` | Claude Code Explore agent |
|---|---|---|
| Right answer at #1 | 8/10 | n/a |
| Right answer in output | 10/10 in the top 3 | 10/10 |
| Time | ~0.8 s per question | 38 s for all 10 |
| Cost | ~1¢ per question | 36k Claude tokens, 9 tool calls |

The Explore agent was a bit more precise and explained what it found. Both `jev` misses at #1 were test code (inline `#[cfg(test)]` modules and fixtures) outranking the implementation. Rewording to "implementation code, not tests, that ..." put the real code at #1 for both, at 0.7 s per rerun.

### Diff scope check on ripgrep

One intended change (bump a buffer size) plus two unrelated ones slipped into the same diff. Both unrelated hunks scored 0.97 and the intended one 0.02, in 0.2 s.

### A 2.7 MB OpenAPI spec (993 endpoints, 751 schemas)

The goal was to find the lease/rental parts of a dealer-management system's API. Most of the relevant schemas have cryptic table codes like `LRCHG`, `LRCON` and `LRBILITM`, with no descriptions. I counted the 48 schemas whose names start with `LR` or `LeaseRental` as the lease/rental set.

| Approach | Hits | Correct | Missed |
|---|---|---|---|
| `grep -i 'lease\|rent'` | 44 | 18 | 30 (all the `LR*` codes; also matches "cur**rent**") |
| `jev find`, first query | 36 | 33 | 15 (7 were `...Request` objects the literal wording excluded) |
| `jev find`, refined query | 58 | 48 | 0 |

It found the cryptic tables from their field names alone. Searching the raw file with 40-line windows also found real hits (1.5 s, ~3.5¢), but 335 chunks of JSON with `{` as the preview isn't useful. Flattening with `jq` and using `--chunk 1` is the pattern that works.

This test also turned up a bug. The first version silently skipped files over 1 MB and reported "0 chunks, exit 1", which looks exactly like "searched and found nothing". Named files are now always searched, skips are reported, and an empty search exits `2`.

### Speed and cost

| Corpus | Tokens | Wall clock | Cost |
|---|---|---|---|
| just `src/` (148 files) | ~250k | 0.7 to 0.8 s | ~1¢ |
| ripgrep, whole repo (224 files) | ~1.2M | 1.8 to 2.3 s | ~5¢ |
| 3-hunk diff | ~900 | 0.2 s | ~0 |

Jev charges $0.042 per million input tokens (output is free). TypeSafe says its rate limits are "adjusting dynamically", so large repos may get slower if limits tighten. The tool retries rather than failing.

## Background

`research/jev-agent-pairing.md` is the research that led here: what TypeSafe documents, what other people had built (several `jgrep` clones and MCP servers appeared the week Jev launched), and which agent use cases have evidence behind them. The short version: an MCP tool needs the text passed as tool arguments, so the agent has to write it out first, which defeats the point. A CLI in a pipe keeps the bulk out of the agent's context entirely.

Not affiliated with TypeSafe.

## Changelog

**0.1.1.** Empty piped input now exits `2` instead of falling back to searching the current directory. After an agent committed its work, the routine `git diff | jev find "a change unrelated to ..."` piped in nothing, and `jev` scanned the entire repo: about 17k chunks and 9M tokens per run, flagging most of the repo as "unrelated". On the first day of real use (Codex agents in a multi-agent setup, about 340 runs), this one fallback was roughly 90% of the tokens spent. `jev` now tells a real pipe (even an empty one) apart from an agent shell with `/dev/null` attached, and still searches the current directory in the second case.

**0.1.0.** First release.

## License

MIT
