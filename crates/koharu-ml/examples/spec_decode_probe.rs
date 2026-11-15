//! Prototype: speculative decoding of the translation model with a small draft.
//!
//! Not wired into the translation path. The probe loads a target GGUF and a
//! draft GGUF from the same tokenizer family, replays one real translation
//! prompt (the exact rendered chat prompt a run produces, dumped to a file),
//! and compares three decoding loops token for token:
//!
//! - `plain` — one target token per step, greedy, the loop `--deterministic`
//!   runs in production;
//! - `spec` — the draft proposes `gamma` tokens, the target verifies them in a
//!   single batched decode, the accepted prefix is kept and one bonus token is
//!   emitted. The loop is built to reproduce `plain` token for token; on Qwen
//!   3.5 it does not always (see `bench-profile/spec-decode.md`).
//! - `builtin` — the same greedy decoding through llama.cpp's own sampler chain
//!   (grammar + penalties + greedy), the reference the manual penalty/RNG-free
//!   sampling above has to reproduce.
//!
//! Run with the llama.cpp runtime on `PATH` (the store keeps it under
//! `store/llama/<build>/windows-cuda`):
//!
//! ```text
//! cargo run --release -p koharu-ml --example spec_decode_probe -- \
//!   --target <4B.gguf> --draft <0.8B.gguf> \
//!   --prompt bench-profile/spec/prompt-000.txt \
//!   --schema bench-profile/spec/schema-000.json --gamma 4 --max-tokens 256
//! ```
//!
//! `--costs` times the building blocks of a round, `--sequential-replay` replays
//! a verified prefix one token at a time instead of in one batch, and
//! `SPEC_DEBUG=1` dumps every round's proposals and verifications.

use std::{
    collections::HashSet,
    num::NonZeroU32,
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use koharu_llama::{
    LlamaStateSeqFlags, SeqState,
    context::{LlamaContext, params::LlamaContextParams},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
    token::{LlamaToken, data::LlamaTokenData, data_array::LlamaTokenDataArray},
};
use llguidance::{Matcher, ParserFactory, api::TopLevelGrammar};

/// Generation settings the `qwen3.5-*` catalog entries use, and the ones the
/// `--deterministic` path builds its sampler chain from.
const REPEAT_LAST_N: i32 = 64;
const REPEAT_PENALTY: f32 = 1.2;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("spec_decode_probe: {error:#}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    target: PathBuf,
    draft: PathBuf,
    prompt: PathBuf,
    schema: Option<PathBuf>,
    gamma: usize,
    max_tokens: usize,
    costs: bool,
    sequential_replay: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut target = None;
        let mut draft = None;
        let mut prompt = None;
        let mut schema = None;
        let mut gamma = 4;
        let mut max_tokens = 256;
        let mut costs = false;
        let mut sequential_replay = false;
        while let Some(flag) = args.next() {
            let mut value = || args.next().with_context(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--target" => target = Some(PathBuf::from(value()?)),
                "--draft" => draft = Some(PathBuf::from(value()?)),
                "--prompt" => prompt = Some(PathBuf::from(value()?)),
                "--schema" => schema = Some(PathBuf::from(value()?)),
                "--gamma" => gamma = value()?.parse().context("--gamma must be a number")?,
                "--max-tokens" => {
                    max_tokens = value()?.parse().context("--max-tokens must be a number")?;
                }
                "--costs" => costs = true,
                "--sequential-replay" => sequential_replay = true,
                other => bail!("unknown argument {other}"),
            }
        }
        Ok(Self {
            target: target.context("--target is required")?,
            draft: draft.context("--draft is required")?,
            prompt: prompt.context("--prompt is required")?,
            schema,
            gamma,
            max_tokens,
            costs,
            sequential_replay,
        })
    }
}

fn run() -> Result<()> {
    let options = Options::parse()?;
    let prompt = std::fs::read_to_string(&options.prompt)
        .with_context(|| format!("failed to read {}", options.prompt.display()))?;
    let schema = match &options.schema {
        Some(path) => Some(with_guidance_hints(
            &std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        )?),
        None => None,
    };

    let backend = LlamaBackend::init().or_else(|error| match error {
        koharu_llama::LlamaCppError::BackendAlreadyInitialized => {
            Ok(LlamaBackend::init().expect("backend initialized"))
        }
        error => Err(error),
    })?;
    let target = Box::leak(Box::new(load(&options.target, &backend)?));
    let draft = Box::leak(Box::new(load(&options.draft, &backend)?));
    println!(
        "target: {} (vocab {}, {} layers, hybrid {}, recurrent {})",
        options.target.display(),
        target.n_vocab(),
        target.n_layer(),
        target.is_hybrid(),
        target.is_recurrent(),
    );
    println!(
        "draft:  {} (vocab {}, {} layers, hybrid {}, recurrent {})",
        options.draft.display(),
        draft.n_vocab(),
        draft.n_layer(),
        draft.is_hybrid(),
        draft.is_recurrent(),
    );
    if target.n_vocab() != draft.n_vocab() {
        bail!(
            "draft vocabulary ({} tokens) differs from the target ({})",
            draft.n_vocab(),
            target.n_vocab()
        );
    }

    let prompt_tokens = target
        .str_to_token(&prompt, AddBos::Always)
        .context("failed to tokenize the prompt")?;
    println!(
        "prompt: {} tokens, gamma {}, cap {} tokens",
        prompt_tokens.len(),
        options.gamma,
        options.max_tokens
    );

    let n_ctx = prompt_tokens
        .len()
        .saturating_add(options.max_tokens)
        .saturating_add(1);
    if options.costs {
        costs(target, draft, &backend, &prompt_tokens, n_ctx)?;
        return Ok(());
    }
    let plain = run_plain(
        target,
        &backend,
        &prompt_tokens,
        n_ctx,
        options.max_tokens,
        schema.as_deref(),
    )?;
    let spec = run_speculative(
        target,
        draft,
        &backend,
        &prompt_tokens,
        n_ctx,
        options.max_tokens,
        options.gamma,
        schema.as_deref(),
        options.sequential_replay,
    )?;
    let builtin = run_builtin(
        target,
        &backend,
        &prompt_tokens,
        n_ctx,
        options.max_tokens,
        schema.as_deref(),
    )?;

    report("plain", &plain, Some(&plain));
    report("spec", &spec, Some(&plain));
    report("builtin", &builtin, Some(&plain));
    Ok(())
}

/// Times the building blocks a speculative round is made of, so the measured
/// speedup can be checked against the arithmetic rather than guessed at.
fn costs(
    model: &LlamaModel,
    draft_model: &LlamaModel,
    backend: &LlamaBackend,
    prompt: &[LlamaToken],
    n_ctx: usize,
) -> Result<()> {
    const REPS: usize = 16;
    let mut target = Side::new(model, backend, n_ctx, None)?;
    let start = Instant::now();
    target.prefill(prompt)?;
    println!(
        "prefill {:>4} tokens: {:>7.1} ms",
        prompt.len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    let next = target.pick_next(&[]);
    let mut position = prompt.len();
    for batch in [1usize, 2, 4, 8] {
        let tokens = vec![next; batch];
        let start = Instant::now();
        for _ in 0..REPS {
            target.decode_many(&tokens, position, true)?;
            position += batch;
        }
        println!(
            "target decode {batch:>2} tokens: {:>7.1} ms/pass ({:.1} ms/token)",
            start.elapsed().as_secs_f64() * 1000.0 / REPS as f64,
            start.elapsed().as_secs_f64() * 1000.0 / (REPS * batch) as f64
        );
    }
    let start = Instant::now();
    let mut bytes = 0;
    for _ in 0..REPS {
        bytes = target.snapshot()?.byte_len();
    }
    println!(
        "target snapshot ({} MiB): {:>7.1} ms",
        bytes / (1024 * 1024),
        start.elapsed().as_secs_f64() * 1000.0 / REPS as f64
    );
    let state = target.snapshot()?;
    let start = Instant::now();
    for _ in 0..REPS {
        target.restore(&state)?;
    }
    println!(
        "target restore: {:>7.1} ms",
        start.elapsed().as_secs_f64() * 1000.0 / REPS as f64
    );

    let mut draft = Side::new(draft_model, backend, n_ctx, None)?;
    draft.prefill(prompt)?;
    let draft_next = draft.pick_next(&[]);
    let start = Instant::now();
    for step in 0..REPS {
        draft.decode(draft_next, prompt.len() + step, true)?;
    }
    println!(
        "draft decode 1 token: {:>7.1} ms",
        start.elapsed().as_secs_f64() * 1000.0 / REPS as f64
    );
    Ok(())
}

/// Mirrors the `x-guidance` block `koharu-ml` injects before compiling the
/// output schema, so the probe generates the same token stream a run does.
fn with_guidance_hints(schema: &str) -> Result<String> {
    let mut schema: serde_json::Value =
        serde_json::from_str(schema).context("output schema is not valid JSON")?;
    if let Some(schema) = schema.as_object_mut() {
        schema.entry("x-guidance").or_insert_with(|| {
            serde_json::json!({
                "item_separator": ", ",
                "key_separator": ": ",
                "whitespace_flexible": false
            })
        });
    }
    Ok(serde_json::to_string(&schema)?)
}

fn load(path: &std::path::Path, backend: &LlamaBackend) -> Result<LlamaModel> {
    let params = LlamaModelParams::default().with_n_gpu_layers(999);
    LlamaModel::load_from_file(backend, path, &params)
        .with_context(|| format!("failed to load {}", path.display()))
}

/// One model, its context, the sampler state the greedy path carries.
struct Side<'a> {
    model: &'a LlamaModel,
    context: LlamaContext<'a>,
    grammar: Option<LlamaSampler>,
    /// Prompt and accepted tokens: the lookback the repeat penalty reads.
    history: Vec<LlamaToken>,
    eos: LlamaToken,
    /// Replay a verified prefix one token at a time instead of in one batch.
    sequential_replay: bool,
}

impl<'a> Side<'a> {
    fn new(
        model: &'a LlamaModel,
        backend: &LlamaBackend,
        n_ctx: usize,
        schema: Option<&str>,
    ) -> Result<Self> {
        let n_ctx = u32::try_from(n_ctx).unwrap_or(u32::MAX);
        let n_ctx = NonZeroU32::new(n_ctx.max(1)).context("n_ctx must be non-zero")?;
        let params = LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx))
            .with_n_batch(n_ctx.get().min(2048))
            .with_n_ubatch(n_ctx.get().min(512));
        let context = model
            .new_context(backend, params)
            .context("failed to create a llama.cpp context")?;
        let grammar = schema
            .map(|schema| grammar_sampler(model, schema))
            .transpose()?;
        Ok(Self {
            model,
            context,
            grammar,
            history: Vec::new(),
            eos: model.token_eos(),
            sequential_replay: false,
        })
    }

    fn prefill(&mut self, tokens: &[LlamaToken]) -> Result<()> {
        let (head, last) = tokens.split_at(tokens.len() - 1);
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (position, token) in head.iter().enumerate() {
            batch
                .add(*token, i32::try_from(position)?, &[0], false)
                .context("failed to build the prompt batch")?;
        }
        batch
            .add(last[0], i32::try_from(tokens.len() - 1)?, &[0], true)
            .context("failed to build the prompt batch")?;
        self.context
            .decode(&mut batch)
            .context("failed to prefill the prompt")?;
        // Only the penalty lookback sees the prompt; the grammar starts at the
        // response, exactly like production does.
        self.history.extend_from_slice(tokens);
        Ok(())
    }

    fn decode(&mut self, token: LlamaToken, position: usize, logits: bool) -> Result<()> {
        let mut batch = LlamaBatch::new(1, 1);
        batch
            .add(token, i32::try_from(position)?, &[0], logits)
            .context("failed to build a decode batch")?;
        self.context
            .decode(&mut batch)
            .context("failed to decode a token")?;
        Ok(())
    }

    fn decode_many(&mut self, tokens: &[LlamaToken], position: usize, logits: bool) -> Result<()> {
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (index, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i32::try_from(position + index)?, &[0], logits)
                .context("failed to build a verification batch")?;
        }
        self.context
            .decode(&mut batch)
            .context("failed to decode a verification batch")?;
        Ok(())
    }

    /// Replays a verified prefix in one pass, asking for the last row's logits
    /// only: the reward for a model whose KV cache cannot be trimmed.
    fn decode_sequence(&mut self, tokens: &[LlamaToken], position: usize) -> Result<()> {
        let (head, last) = tokens.split_at(tokens.len() - 1);
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (index, token) in head.iter().enumerate() {
            batch
                .add(*token, i32::try_from(position + index)?, &[0], false)
                .context("failed to build a replay batch")?;
        }
        batch
            .add(
                last[0],
                i32::try_from(position + tokens.len() - 1)?,
                &[0],
                true,
            )
            .context("failed to build a replay batch")?;
        self.context.decode(&mut batch).with_context(|| {
            format!(
                "failed to replay a verified prefix ({} tokens at {position})",
                tokens.len()
            )
        })?;
        Ok(())
    }

    fn snapshot(&self) -> Result<SeqState> {
        self.context
            .state_seq_get(0, LlamaStateSeqFlags::empty())
            .context("failed to snapshot the sequence state")
    }

    fn restore(&mut self, state: &SeqState) -> Result<()> {
        self.context
            .state_seq_set(state, 0)
            .context("failed to restore the sequence state")
    }

    /// Greedy pick over `logits`, with the repeat penalty rebuilt from the
    /// lookback `history ++ extra` and the grammar applied as a mask.
    ///
    /// `extra` is the set of tokens the plain loop would have accepted before
    /// this position, which is what makes a batched verification reproduce the
    /// one-token-at-a-time loop bit for bit.
    fn pick(&self, logits: &[f32], extra: &[LlamaToken]) -> LlamaToken {
        let mut penalized = HashSet::new();
        let window = usize::try_from(REPEAT_LAST_N.max(0)).unwrap_or(usize::MAX);
        for token in self
            .history
            .iter()
            .rev()
            .copied()
            .chain(extra.iter().rev().copied())
            .take(window)
        {
            penalized.insert(token.0);
        }

        let mut data = Vec::with_capacity(logits.len());
        for (id, logit) in logits.iter().enumerate() {
            let id = LlamaToken::new(i32::try_from(id).expect("vocabulary does not fit an i32"));
            let logit = if penalized.contains(&id.0) {
                if *logit > 0.0 {
                    logit / REPEAT_PENALTY
                } else {
                    logit * REPEAT_PENALTY
                }
            } else {
                *logit
            };
            data.push(LlamaTokenData::new(id, logit, 0.0));
        }
        let mut data = LlamaTokenDataArray::new(data, false);
        if let Some(grammar) = &self.grammar {
            grammar.apply(&mut data);
        }
        data.data
            .iter()
            .max_by(|left, right| left.logit().total_cmp(&right.logit()))
            .map(LlamaTokenData::id)
            .expect("the vocabulary is never empty")
    }

    /// Picks the next token from the context's newest logits row.
    fn pick_next(&self, extra: &[LlamaToken]) -> LlamaToken {
        self.pick(self.context.get_logits(), extra)
    }

    fn accept(&mut self, token: LlamaToken) {
        if let Some(grammar) = &mut self.grammar {
            grammar.accept(token);
        }
        self.history.push(token);
    }

    fn is_stop(&self, token: LlamaToken) -> bool {
        token == self.eos || self.model.is_eog_token(token)
    }

    fn rollback(&mut self, keep: usize) -> Result<()> {
        self.context
            .kv_cache_seq_rm(0, Some(u32::try_from(keep)?), None)
            .context("failed to roll the KV cache back")?;
        self.history.truncate(keep);
        Ok(())
    }

    /// Puts the context back on the verified prefix and advances it to the end
    /// of the emitted tokens, so both models start the next round from the same
    /// state. Returns how many rows were decoded, for the report.
    fn advance(
        &mut self,
        snapshot: Option<&SeqState>,
        emitted: &[LlamaToken],
        position: usize,
        matched: usize,
        gamma: usize,
        prefix_accepted: bool,
    ) -> Result<usize> {
        let bonus = *emitted.last().expect("emitted always ends with the bonus");
        if !prefix_accepted {
            for token in &emitted[..emitted.len() - 1] {
                self.accept(*token);
            }
        }
        if matched == gamma {
            // The cache already holds every accepted proposal.
            self.accept(bonus);
            self.decode(bonus, position + gamma, true)?;
            return Ok(1);
        }
        match snapshot {
            Some(state) => {
                // Restore the pre-verification state, then replay the prefix a
                // model with a retractable cache could have kept in place.
                self.restore(state)?;
                self.accept(bonus);
                if self.sequential_replay {
                    for (index, token) in emitted.iter().enumerate() {
                        self.decode(*token, position + index, index + 1 == emitted.len())?;
                    }
                } else {
                    self.decode_sequence(emitted, position)?;
                }
                Ok(emitted.len())
            }
            None => {
                self.rollback(position + matched)?;
                self.accept(bonus);
                self.decode(bonus, position + matched, true)?;
                Ok(1)
            }
        }
    }
}

fn grammar_sampler(model: &LlamaModel, schema: &str) -> Result<LlamaSampler> {
    let tok_env = LlamaSampler::llguidance_tok_env(model);
    let factory =
        ParserFactory::new_simple(&tok_env).map_err(|error| anyhow::anyhow!("{error}"))?;
    let grammar = TopLevelGrammar::from_tagged_str("json_schema", schema)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let parser = factory
        .create_parser(grammar)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(LlamaSampler::from(Matcher::new(Ok(parser))))
}

/// llama.cpp's own greedy chain, the reference the manual sampler reproduces.
fn builtin_sampler(
    model: &LlamaModel,
    history: &[LlamaToken],
    schema: Option<&str>,
) -> Result<LlamaSampler> {
    let mut samplers = Vec::new();
    if let Some(schema) = schema {
        samplers.push(grammar_sampler(model, schema)?);
    }
    let mut penalties = LlamaSampler::penalties(model, REPEAT_LAST_N, REPEAT_PENALTY, 0.0, 0.0);
    penalties.accept_many(history);
    samplers.push(penalties);
    samplers.push(LlamaSampler::greedy());
    Ok(LlamaSampler::chain_simple(samplers))
}

struct Outcome {
    tokens: Vec<LlamaToken>,
    text: String,
    generation: Duration,
    target_decodes: usize,
    target_rows: usize,
    draft_decodes: usize,
    accepted: usize,
    proposed: usize,
    rounds: usize,
}

fn decode_text(model: &LlamaModel, tokens: &[LlamaToken]) -> String {
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut text = String::new();
    for token in tokens {
        if let Ok(piece) = model.token_to_piece(*token, &mut decoder, true, None) {
            text.push_str(&piece);
        }
    }
    text
}

fn run_plain(
    model: &LlamaModel,
    backend: &LlamaBackend,
    prompt: &[LlamaToken],
    n_ctx: usize,
    max_tokens: usize,
    schema: Option<&str>,
) -> Result<Outcome> {
    let mut side = Side::new(model, backend, n_ctx, schema)?;
    side.prefill(prompt)?;
    let mut position = prompt.len();
    let mut tokens = Vec::new();
    let start = Instant::now();
    loop {
        let token = side.pick_next(&[]);
        if side.is_stop(token) || tokens.len() >= max_tokens {
            break;
        }
        tokens.push(token);
        side.accept(token);
        if tokens.len() >= max_tokens {
            break;
        }
        side.decode(token, position, true)?;
        position += 1;
    }
    let generation = start.elapsed();
    Ok(Outcome {
        text: decode_text(model, &tokens),
        target_decodes: tokens.len(),
        target_rows: tokens.len(),
        tokens,
        generation,
        draft_decodes: 0,
        accepted: 0,
        proposed: 0,
        rounds: 0,
    })
}

fn run_builtin(
    model: &LlamaModel,
    backend: &LlamaBackend,
    prompt: &[LlamaToken],
    n_ctx: usize,
    max_tokens: usize,
    schema: Option<&str>,
) -> Result<Outcome> {
    let mut side = Side::new(model, backend, n_ctx, schema)?;
    side.prefill(prompt)?;
    let mut sampler = builtin_sampler(model, &side.history, schema)?;
    let mut position = prompt.len();
    let mut tokens = Vec::new();
    let start = Instant::now();
    loop {
        let token = sampler.sample(&side.context, -1);
        if side.is_stop(token) || tokens.len() >= max_tokens {
            break;
        }
        tokens.push(token);
        if tokens.len() >= max_tokens {
            break;
        }
        side.decode(token, position, true)?;
        position += 1;
    }
    let generation = start.elapsed();
    Ok(Outcome {
        text: decode_text(model, &tokens),
        target_decodes: tokens.len(),
        target_rows: tokens.len(),
        tokens,
        generation,
        draft_decodes: 0,
        accepted: 0,
        proposed: 0,
        rounds: 0,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_speculative(
    model: &LlamaModel,
    draft_model: &LlamaModel,
    backend: &LlamaBackend,
    prompt: &[LlamaToken],
    n_ctx: usize,
    max_tokens: usize,
    gamma: usize,
    schema: Option<&str>,
    sequential_replay: bool,
) -> Result<Outcome> {
    let mut target = Side::new(model, backend, n_ctx, schema)?;
    let mut draft = Side::new(draft_model, backend, n_ctx, None)?;
    target.sequential_replay = sequential_replay;
    draft.sequential_replay = sequential_replay;
    target.prefill(prompt)?;
    draft.prefill(prompt)?;

    let mut tokens: Vec<LlamaToken> = Vec::new();
    let mut position = prompt.len();
    let (mut accepted, mut proposed, mut rounds) = (0, 0, 0);
    let (mut target_decodes, mut target_rows, mut draft_decodes, mut draft_rows) = (0, 0, 0, 0);
    let mut snapshot_bytes = 0;
    let start = Instant::now();

    'outer: loop {
        rounds += 1;
        // A hybrid model (Qwen 3.5 carries a linear-attention state) refuses a
        // partial KV removal, so the sequence state is captured at the round's
        // starting position and restored when a proposal is rejected.
        let target_snapshot = if model.is_hybrid() {
            Some(target.snapshot()?)
        } else {
            None
        };
        let draft_snapshot = if draft_model.is_hybrid() {
            Some(draft.snapshot()?)
        } else {
            None
        };

        // The draft proposes gamma tokens, reading its own lookback plus the
        // proposals it already made this round.
        let mut proposals = Vec::with_capacity(gamma);
        let mut token = draft.pick_next(&[]);
        for index in 0..gamma {
            proposals.push(token);
            draft.decode(token, position + index, true)?;
            draft_decodes += 1;
            if index + 1 < gamma {
                token = draft.pick_next(&proposals);
            }
        }
        proposed += gamma;

        // Verify the whole draft against the target in one batched decode. The
        // row that predicts `position` predates the batch, so it is saved first.
        let before = target.context.get_logits().to_vec();
        target.decode_many(&proposals, position, true)?;
        target_decodes += 1;
        target_rows += gamma;
        // Verify row by row and accept each match into the sampler as it is
        // confirmed: the grammar and the penalty lookback then sit exactly where
        // the plain loop keeps them, with nothing to roll back when a proposal
        // is rejected.
        let mut verify = Vec::with_capacity(gamma + 1);
        let mut expected = target.pick(&before, &[]);
        verify.push(expected);
        let mut matched = 0;
        while matched < gamma && proposals[matched] == expected {
            target.accept(proposals[matched]);
            matched += 1;
            if matched < gamma {
                let logits = target
                    .context
                    .get_logits_ith(i32::try_from(matched - 1).expect("gamma fits an i32"));
                expected = target.pick(logits, &[]);
                verify.push(expected);
            }
        }
        if matched == gamma {
            let logits = target
                .context
                .get_logits_ith(i32::try_from(gamma - 1).expect("gamma fits an i32"));
            expected = target.pick(logits, &[]);
            verify.push(expected);
        }
        accepted += matched;
        let bonus = expected;
        if std::env::var_os("SPEC_DEBUG").is_some() {
            eprintln!(
                "ROUND {rounds} pos={position} matched={matched} proposals={:?} verify={:?}",
                proposals.iter().map(|t| t.0).collect::<Vec<_>>(),
                verify.iter().map(|t| t.0).collect::<Vec<_>>()
            );
        }

        // The tokens the plain loop would emit for this round, in order: the
        // matched proposals, then the correction the target itself picked.
        let stop = target.is_stop(bonus);
        let mut emitted = proposals[..matched].to_vec();
        if !stop {
            emitted.push(bonus);
        }
        let room = max_tokens.saturating_sub(tokens.len());
        let full = emitted.len() > room;
        if full {
            emitted.truncate(room);
        }
        tokens.extend_from_slice(&emitted);
        if stop || full || emitted.is_empty() {
            break 'outer;
        }

        target_rows += gamma;
        target_rows += target.advance(
            target_snapshot.as_ref(),
            &emitted,
            position,
            matched,
            gamma,
            true,
        )?;
        draft_rows += draft.advance(
            draft_snapshot.as_ref(),
            &emitted,
            position,
            matched,
            gamma,
            false,
        )?;
        target_decodes += 1;
        draft_decodes += 1;
        position += emitted.len();
        if let Some(snapshot) = &target_snapshot {
            snapshot_bytes = snapshot.byte_len();
        }
    }

    let generation = start.elapsed();
    tokens.truncate(max_tokens);
    if snapshot_bytes > 0 {
        println!("         sequence state snapshot: {snapshot_bytes} bytes");
    }
    let _ = draft_rows;
    Ok(Outcome {
        text: decode_text(model, &tokens),
        tokens,
        generation,
        target_decodes,
        target_rows,
        draft_decodes,
        accepted,
        proposed,
        rounds,
    })
}

fn report(name: &str, outcome: &Outcome, baseline: Option<&Outcome>) {
    let rate = outcome.tokens.len() as f64 / outcome.generation.as_secs_f64();
    println!(
        "{name:<8} {:>4} tokens  {:>7.2} s  {:>6.2} tok/s  target decodes {:>3} ({} rows)  draft {:>3}  accepted {}/{} ({:.0}%)  rounds {}",
        outcome.tokens.len(),
        outcome.generation.as_secs_f64(),
        rate,
        outcome.target_decodes,
        outcome.target_rows,
        outcome.draft_decodes,
        outcome.accepted,
        outcome.proposed,
        if outcome.proposed == 0 {
            0.0
        } else {
            100.0 * outcome.accepted as f64 / outcome.proposed as f64
        },
        outcome.rounds,
    );
    let accepted = if outcome.proposed == 0 {
        0.0
    } else {
        100.0 * outcome.accepted as f64 / outcome.proposed as f64
    };
    // name, tokens, seconds, tokens/second, accepted %, accepted, proposed, rounds
    println!(
        "CSV,{name},{},{:.3},{:.2},{accepted:.0},{},{},{}",
        outcome.tokens.len(),
        outcome.generation.as_secs_f64(),
        outcome.tokens.len() as f64 / outcome.generation.as_secs_f64(),
        outcome.accepted,
        outcome.proposed,
        outcome.rounds,
    );
    let baseline = baseline.filter(|_| name != "plain");
    let Some(baseline) = baseline else {
        return;
    };
    let identical = outcome.tokens == baseline.tokens;
    let speedup = baseline.generation.as_secs_f64() / outcome.generation.as_secs_f64();
    let divergence = outcome
        .tokens
        .iter()
        .zip(baseline.tokens.iter())
        .position(|(left, right)| left != right);
    println!(
        "         text {} chars, identical {}{}, {speedup:.2}x plain",
        outcome.text.chars().count(),
        identical,
        match divergence {
            Some(index) if !identical => format!(", first divergence at token {index}"),
            _ => String::new(),
        }
    );
}
