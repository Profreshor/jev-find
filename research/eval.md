# Eval: `jev find` (2026-09-25)

## casey/just vs an Explore agent

Repo: casey/just, `src/` only (148 files, ~28k lines). 10 "where is X" queries phrased by behavior, no identifiers.

| # | Query | jev #1 correct | jev top-3 correct | Explore correct |
|---|---|---|---|---|
| 1 | circular recipe dependencies | yes (recipe_resolver.rs:105) | yes | yes |
| 2 | suggest similar recipe name | yes (justfile.rs:66) | yes | yes |
| 3 | load .env file | yes (load_dotenv.rs:141) | yes | yes |
| 4 | search parent dirs for justfile | related fn (search.rs:164) | yes (search.rs:233) | yes |
| 5 | pick shell for recipe lines | yes (settings.rs:67) | yes | yes |
| 6 | reject mixed tabs/spaces | yes (indentation.rs:39) | yes | yes (lexer.rs:445, more precise) |
| 7 | backtick evaluation | yes (evaluator.rs:512) | yes | yes |
| 8 | list recipes with doc comments | yes (subcommand.rs:649) | yes | yes |
| 9 | run shebang script | no, test fixtures first | yes (recipe.rs:511) | yes |
| 10 | Ctrl-C during recipe | yes (recipe.rs:732) | yes (signal_handler.rs next best) | yes |

jev: 8/10 at #1, 10/10 in top 3. ~0.8s and ~250k Jev tokens (~$0.01) per query.
Explore (one agent, all 10 batched): 10/10, 38s, 35.7k Claude tokens, 9 tool calls.

Takeaways: Explore is a bit more precise and explains what it found. jev is ~40x faster per question, needs no subagent, and
scales linearly with repo size. Test code that performs the described behavior is the main noise source; narrowing paths helps.
Diff scope check (ripgrep, 1 intended + 2 unrelated hunks): unrelated 0.97/0.97, intended 0.02.

Follow-up: the two misses came from inline `#[cfg(test)]` modules and fixtures inside `src/`, so narrowing paths wouldn't help.
Rewording to "implementation code, not tests, that ..." put the real implementation at #1 for both (#4: search.rs:233; #9: recipe.rs:692/511), 0.7s each.

## Large OpenAPI spec (a dealer-management system API, 2.7 MB, 993 paths, 751 schemas)

- Bug found: files over 1 MB were silently skipped, giving "0 chunks, exit 1" (looks like "not found"). Fixed: named files are never size-skipped; skips are reported; nothing-to-search exits 2.
- Raw file with 40-line chunks: 1.5s, 820k tokens, 335 matching chunks. Real hits, but useless previews.
- One line per schema via jq + `--chunk 1` (462 schemas, 64k tokens, 0.5s). Ground truth: 48 schemas named LR*/LeaseRental*.
  - grep 'lease|rent': 44 hits, only 18 correct (matches "cur-rent" etc.), missed 30 cryptic LR* tables.
  - jev, first query ("database table for leasing/rental..."): 36 hits, 33 correct, missed 15 (7 were Request objects the literal wording excluded).
  - jev, refined query: 58 hits, all 48 found, 10 extras (some defensible: recurring charges, VehicleSale.Lease).
