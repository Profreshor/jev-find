---
name: jev
description: 'Find code, log lines, diff hunks, or records by what they do, in about a second: `jev find "<description>" [paths]`. Use it instead of guessing grep patterns or starting an Explore/search subagent when asking "where is X handled?" without knowing the identifier names; when command output is too long to read and grep won''t isolate the cause; when a large JSON/OpenAPI/CSV file has cryptic names; and before reporting a multi-file change done, to check your diff for unrelated changes.'
---

# jev find

`jev find "<description>" [paths]` scores every chunk of text (~40 lines) for whether it matches a plain-English description and prints the matches, best first. Cost scales with how much text you search: ~150 files is about a cent, ~4,000 files is about 40 cents. Always pass the one or two directories where the behavior should live, never `.` in a large repo. Searches over ~1M tokens (roughly 2,000 chunks) are refused, and there is a daily budget shared by every agent on the machine. Piped stdin is searched instead of files.

## When to reach for it

- **Unfamiliar codebase, you know the behavior but not the name.** Run this before guessing grep patterns or spawning an explore agent: `jev find "retries failed HTTP requests with backoff" src`. Then Read the ranges it prints.
- **Long output where grep won't find the cause.** `cargo test 2>&1 | jev find "explains why a test failed"`. Piped input comes back as text, since you can't re-read it later.
- **Scope check before saying a multi-file change is done.** `git diff | jev find "a change unrelated to <the task>"`, or `git diff <base>` if you already committed. Diff only your own change: a diff of hundreds of hunks means the wrong base. Each hunk is scored separately and labelled with its file. In your report, say whether each flagged hunk was intended; a flag nobody reads is wasted. Empty piped input is an error (exit 2), not a repo search.
- **Big structured files (OpenAPI specs, JSON dumps, CSVs).** 40-line windows of JSON make poor results. Flatten to one line per item with `jq` and use `--chunk 1`, so each item is scored on its own:
  `jq -r '.components.schemas|to_entries[]|"\(.key): \((.value.properties//{})|keys|join(", "))"' spec.json | jev find "lease or rental tables" --chunk 1 --top 100`
  This finds items with cryptic names (e.g. `LRCHG`) that keyword grep misses, working from their field names.

Don't use it when you know the identifier (grep is exact and free), for small repos you can just read, or for counting or arithmetic ("how many X"), which Jev can't do.

## Writing the description

Jev reads the description literally. Describe the content you want to see, not the question you have:

- Good: `"parses a single line of a gitignore file into a glob pattern"`
- Bad: `"how does gitignore work?"`, `"gitignore"` (use grep for keywords)

For negatives, state the thing to find: `"a change that does not relate to <task>"`.

**Refine and rerun on narrow paths.** Rewording and rerunning is fine when the paths are small. If a search was broad, narrow the paths before rerunning, or switch to grep once a result gives you an identifier. If tests or docs outrank the implementation, add the exclusion to the description: `"implementation code, not tests, that <behavior>"`. On casey/just that moved the real code from #3 to #1 on both queries where tests had been winning.

## Reading the output

```
src/net.rs:40-79  0.94  fn retry_with_backoff(...)      <- path:lines  probability  first line
jev: 3 of 412 chunks >= 0.5; next best 0.31 at src/x.rs:1-40 | ...   (stderr)
```

- Scores are independent probabilities, not ranks. Several chunks above 0.9 is normal for a common behavior.
- **Read the summary line**, and never redirect stderr (`2>/dev/null`) or you lose it. "0 matches, next best 0.45" means look there. "Next best 0.03" means it's not in these paths, which is real evidence of absence.
- Test code and docs that describe the behavior also score high. Say "not tests" in the description (see above), or narrow the paths when tests live in a separate directory.
- Exit code: 0 found matches, 1 found none, 2 error.

Flags: `--top N` (default 20), `--threshold P` (default 0.5), `--chunk LINES` (default 40), `--files` (one line per file, which ranks files by relevance), `--json`, `--max-tokens N` (default 1,000,000).

**If it refuses** (exit 2, "refusing: ..."), don't raise `--max-tokens` to get past it. Narrow the paths, or use grep. When the daily budget is spent, or jev says it is disabled for this agent, stop using jev for the session.

Respects `.gitignore`. When walking directories it skips binary files and files over 1 MB, and reports the skip count in the summary. A file you name directly is always searched. Needs `TYPESAFE_API_KEY` in the environment, `./.env`, or `~/.config/jev/.env`.
