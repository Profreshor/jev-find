use std::collections::HashMap;
use std::io::{IsTerminal, Read};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

const URL: &str = "https://api.typesafe.ai/v1/systemone";
// ponytail: fixed window budget, keeps state small (Jev loses accuracy on large state). Tune if eval says so.
const WINDOW_CHARS: usize = 24_000;
const WINDOW_CHUNKS: usize = 64;
const CHUNK_CHARS: usize = 6_000;
const MAX_FILE_BYTES: u64 = 1_000_000;
const PARALLEL: usize = 16;

#[derive(Parser)]
#[command(about = "Semantic search powered by TypeSafe Jev")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find chunks of text or code that match a plain-English description.
    /// Reads stdin when piped, otherwise searches PATHS (default: current dir, respecting .gitignore).
    Find {
        /// What you're looking for, e.g. "retries failed HTTP requests with backoff"
        query: String,
        paths: Vec<String>,
        /// Max lines per chunk
        #[arg(long, default_value_t = 40)]
        chunk: usize,
        /// Minimum probability to count as a match
        #[arg(long, default_value_t = 0.5)]
        threshold: f64,
        /// Max results to print
        #[arg(long, default_value_t = 20)]
        top: usize,
        /// One line per file (best chunk score) instead of per chunk
        #[arg(long)]
        files: bool,
        #[arg(long)]
        json: bool,
    },
}

struct Chunk {
    src: String,
    start: usize, // 1-indexed, inclusive
    end: usize,
    text: String,
}

/// Split into chunks of at most `max` lines. Diff hunks always get their own chunk;
/// otherwise prefer cutting after a blank line so functions stay whole where possible.
fn split(lines: &[&str], max: usize) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start = 0;
    while start < lines.len() {
        let hard = (start + max).min(lines.len());
        // Next file or hunk header, unless this chunk is still in the header leading up to its first hunk.
        let forced = (start + 1..hard).find(|&i| {
            lines[i].starts_with("diff --git")
                || (lines[i].starts_with("@@") && lines[start..i].iter().any(|l| l.starts_with("@@")))
        });
        let end = if let Some(i) = forced {
            i
        } else if hard == lines.len() {
            hard
        } else {
            (start + max / 2 + 1..=hard).rev().find(|&i| lines[i - 1].trim().is_empty()).unwrap_or(hard)
        };
        out.push((start, end));
        start = end;
    }
    out
}

fn chunk_text(src: &str, text: &str, max: usize) -> Vec<Chunk> {
    let lines: Vec<&str> = text.lines().collect();
    split(&lines, max.max(1))
        .into_iter()
        .filter_map(|(s, e)| {
            let mut body = lines[s..e].join("\n");
            if body.trim().is_empty() {
                return None;
            }
            if body.len() > CHUNK_CHARS {
                let cut = (0..=CHUNK_CHARS).rev().find(|&i| body.is_char_boundary(i)).unwrap();
                body.truncate(cut);
            }
            // In a diff, name the file the hunk belongs to.
            let file = lines[..e].iter().rev().find_map(|l| l.strip_prefix("+++ b/"));
            let src = file.map_or(src.to_string(), |f| format!("{src}:{f}"));
            Some(Chunk { src, start: s + 1, end: e, text: body })
        })
        .collect()
}

/// Returns (chunks, files searched, large files skipped). Files named directly are never size-skipped.
fn walk(paths: &[String], max: usize) -> (Vec<Chunk>, usize, usize) {
    let mut chunks = vec![];
    let (mut files, mut skipped) = (0, 0);
    for p in paths {
        for entry in ignore::WalkBuilder::new(p).build().flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if entry.depth() > 0 && entry.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
                skipped += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else { continue };
            if bytes[..bytes.len().min(8192)].contains(&0) {
                continue; // binary
            }
            let path = entry.path().to_string_lossy();
            let path = path.strip_prefix("./").unwrap_or(&path);
            chunks.extend(chunk_text(path, &String::from_utf8_lossy(&bytes), max));
            files += 1;
        }
    }
    (chunks, files, skipped)
}

/// POST with retry on 429/529/5xx and network errors.
async fn post(client: &reqwest::Client, key: &str, body: &Value) -> Result<Value, String> {
    let mut delay = Duration::from_millis(500);
    for attempt in 0..5 {
        let last = attempt == 4;
        match client.post(URL).bearer_auth(key).json(body).send().await {
            Ok(r) if r.status().is_success() => return r.json().await.map_err(|e| e.to_string()),
            Ok(r) => {
                let s = r.status();
                if last || !(s.as_u16() == 429 || s.is_server_error()) {
                    return Err(format!("{s}: {}", r.text().await.unwrap_or_default()));
                }
                if let Some(secs) = r.headers().get("retry-after").and_then(|v| v.to_str().ok()?.parse().ok()) {
                    delay = Duration::from_secs_f64(secs);
                }
            }
            Err(e) if last => return Err(e.to_string()),
            Err(_) => {}
        }
        tokio::time::sleep(delay).await;
        delay *= 2;
    }
    unreachable!()
}

/// Score every chunk with one Noul each. Chunks are packed into windows that share one state.
async fn score(chunks: &[Chunk], query: &str, key: String) -> Result<(Vec<f64>, u64), String> {
    let mut windows: Vec<(usize, usize)> = vec![];
    let (mut start, mut chars) = (0, 0);
    for (i, c) in chunks.iter().enumerate() {
        if i > start && (chars + c.text.len() > WINDOW_CHARS || i - start >= WINDOW_CHUNKS) {
            windows.push((start, i));
            (start, chars) = (i, 0);
        }
        chars += c.text.len();
    }
    if start < chunks.len() {
        windows.push((start, chunks.len()));
    }

    let client = reqwest::Client::new();
    let sem = Arc::new(Semaphore::new(PARALLEL));
    let key = Arc::new(key);
    let mut set = tokio::task::JoinSet::new();
    for (ws, we) in windows {
        let (mut state, mut questions) = (serde_json::Map::new(), serde_json::Map::new());
        for (j, c) in chunks[ws..we].iter().enumerate() {
            let id = format!("c{j}");
            state.insert(id.clone(), json!(format!("[{} lines {}-{}]\n{}", c.src, c.start, c.end, c.text)));
            questions.insert(id.clone(), json!({
                "type": "noul",
                "instructions": format!("Does `chunks.{id}` contain content that matches this description: \"{query}\"?"),
            }));
        }
        let body = json!({ "state": { "chunks": state }, "model": "jev-latest", "questions": questions });
        let (client, sem, key) = (client.clone(), sem.clone(), key.clone());
        set.spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            post(&client, &key, &body).await.map(|r| (ws, we, r))
        });
    }

    let mut scores = vec![0.0; chunks.len()];
    let mut tokens = 0;
    while let Some(res) = set.join_next().await {
        let (ws, we, r) = res.map_err(|e| e.to_string())??;
        tokens += r["usage"]["input_tokens"].as_u64().unwrap_or(0);
        for i in ws..we {
            scores[i] = r["answers"][format!("c{}", i - ws)]["noul"]
                .as_f64()
                .ok_or_else(|| format!("bad response: {r}"))?;
        }
    }
    Ok((scores, tokens))
}

#[tokio::main]
async fn main() {
    let Cmd::Find { query, paths, chunk, threshold, top, files, json } = Cli::parse().cmd;
    // Env var, then ./.env, then ~/.config/jev/.env (agents don't all load shell profiles).
    let _ = dotenvy::dotenv();
    let _ = std::env::var("HOME").map(|h| dotenvy::from_path(format!("{h}/.config/jev/.env")));
    let Ok(key) = std::env::var("TYPESAFE_API_KEY") else {
        eprintln!("jev: TYPESAFE_API_KEY is not set (env, ./.env, or ~/.config/jev/.env)");
        std::process::exit(2);
    };

    // Piped input wins; an empty pipe (common when agents run commands) falls back to searching the cwd.
    let mut piped = String::new();
    if paths.is_empty() && !std::io::stdin().is_terminal() {
        let _ = std::io::stdin().read_to_string(&mut piped);
    }
    let from_stdin = !piped.trim().is_empty();
    let paths = if paths.is_empty() { vec![".".to_string()] } else { paths };
    let (chunks, n_files, skipped) =
        if from_stdin { (chunk_text("stdin", &piped, chunk), 1, 0) } else { walk(&paths, chunk) };
    let skipped_note = if skipped > 0 { format!(", {skipped} files over 1 MB skipped (name them directly to search)") } else { String::new() };
    if chunks.is_empty() {
        eprintln!("jev: nothing to search in {paths:?}{skipped_note}");
        std::process::exit(2);
    }

    let t = Instant::now();
    let (scores, tokens) = match score(&chunks, &query, key).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("jev: request failed: {e}");
            std::process::exit(2);
        }
    };
    let secs = t.elapsed().as_secs_f64();

    // Rows: (score, src, start, end, chunk index). In --files mode keep each file's best chunk.
    let mut rows: Vec<(f64, &str, usize, usize, usize)> =
        chunks.iter().enumerate().map(|(i, c)| (scores[i], c.src.as_str(), c.start, c.end, i)).collect();
    if files {
        let mut best: HashMap<&str, (f64, &str, usize, usize, usize)> = HashMap::new();
        for r in rows {
            let e = best.entry(r.1).or_insert(r);
            if r.0 > e.0 {
                *e = r;
            }
        }
        rows = best.into_values().collect();
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let shown: Vec<_> = rows.iter().take_while(|r| r.0 >= threshold).take(top).collect();
    let next = rows.get(shown.len());
    let n_matches = rows.iter().filter(|r| r.0 >= threshold).count();

    if json {
        let row = |r: &(f64, &str, usize, usize, usize)| {
            let mut v = json!({ "path": r.1, "start": r.2, "end": r.3, "score": r.0 });
            if from_stdin {
                v["text"] = json!(chunks[r.4].text);
            }
            v
        };
        let out = json!({
            "matches": shown.iter().map(|r| row(r)).collect::<Vec<_>>(),
            "total_matches": n_matches,
            "next_best": next.map(row),
            "chunks": chunks.len(), "files": n_files, "skipped_large_files": skipped, "input_tokens": tokens, "seconds": secs,
        });
        println!("{out}");
    } else {
        for r in &shown {
            if from_stdin {
                println!("--- {} lines {}-{}  {:.2}\n{}", r.1, r.2, r.3, r.0, chunks[r.4].text);
            } else {
                let first = chunks[r.4].text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
                let first: String = first.chars().take(90).collect();
                let loc = if files { r.1.to_string() } else { format!("{}:{}-{}", r.1, r.2, r.3) };
                println!("{loc}  {:.2}  {first}", r.0);
            }
        }
        let next = next.map_or(String::new(), |r| format!("; next best {:.2} at {}:{}-{}", r.0, r.1, r.2, r.3));
        eprintln!(
            "jev: {n_matches} of {} {} >= {threshold}{}{next} | {} chunks, {n_files} files{skipped_note}, {tokens} tokens, {secs:.1}s",
            rows.len(),
            if files { "files" } else { "chunks" },
            if n_matches > shown.len() { format!(" (showing {})", shown.len()) } else { String::new() },
            chunks.len(),
        );
    }
    std::process::exit(if n_matches > 0 { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_prefers_blank_lines() {
        let lines: Vec<&str> = "a\nb\nc\n\nd\ne\nf\ng".lines().collect();
        assert_eq!(split(&lines, 6), vec![(0, 4), (4, 8)]);
        // no break point in the back half -> hard cut
        let flat: Vec<&str> = "1\n2\n3\n4\n5".lines().collect();
        assert_eq!(split(&flat, 2), vec![(0, 2), (2, 4), (4, 5)]);
    }

    #[test]
    fn diff_hunks_are_separate_chunks_labelled_by_file() {
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n@@ -9 +9 @@\n-c\n+d\n\
                    diff --git a/y b/y\n+++ b/y\n@@ -1 +1 @@\n-e";
        let c = chunk_text("stdin", diff, 40);
        let got: Vec<_> = c.iter().map(|c| (c.src.as_str(), c.start, c.end)).collect();
        assert_eq!(got, vec![("stdin:x", 1, 6), ("stdin:x", 7, 9), ("stdin:y", 10, 13)]);
    }

    #[test]
    fn chunks_skip_blank_and_are_one_indexed() {
        let c = chunk_text("f", "x\n\n\n\ny", 2);
        let spans: Vec<_> = c.iter().map(|c| (c.start, c.end)).collect();
        assert_eq!(spans, vec![(1, 2), (5, 5)]);
    }
}
