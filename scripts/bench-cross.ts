#!/usr/bin/env bun
// Cross-over wall-time benchmark for koharu-batch: runs a "before" and an
// "after" binary on the same input in BOTH orders (before→after, then
// after→before) so each binary occupies each position once. Averaging every
// binary over its two runs cancels the first-run GPU warm-up bias that a
// single execution order cannot separate from the binary difference — the
// method behind the « banc croisé » numbers recorded in memory.md.
//
//   bun scripts/bench-cross.ts --before <old-bin> --after <new-bin> \
//     --input <cbz-or-dir> [--store <dir>] [--workdir <dir>] \
//     [--orders both|1|2] [--pause <seconds>] [--plan] [-- extra koharu-batch args]
//
// Every run passes --no-calibration (AGENTS.md): the machine's calibration
// file is never read or written. The model resolved by a dry run (both
// binaries must agree) is then pinned with --llm/--quantization for every
// run: `auto` picks by free VRAM, which drifts during a session and would
// silently switch models halfway through the comparison.

import { spawn, spawnSync } from 'node:child_process'
import { createWriteStream, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { parseArgs } from 'node:util'

interface Options {
  before: string
  after: string
  input: string
  store?: string
  workdir: string
  orders: 'both' | '1' | '2'
  pause: number
  plan: boolean
  extra: string[]
  /// `--llm`/`--quantization` pair chosen by checkModels(), forwarded to every run.
  pin: string[]
}

interface RunSpec {
  tag: string
  order: number
  position: number
  binary: 'before' | 'after'
  bin: string
}

interface RunResult {
  spec: RunSpec
  wall: number
  endedAt: string
  reportSeconds: number
  stageSeconds: number
  translated: number
  skipped: number
  failed: number
  model: string
}

const USAGE = `Cross-over (two-order) wall-time benchmark for koharu-batch.

  bun scripts/bench-cross.ts --before <old-bin> --after <new-bin> --input <cbz|dir> [options]

Options:
  --before <path>    Binary at the baseline commit
  --after <path>     Binary under test
  --input <path>     Folder of pages or .cbz archive (same input for all runs)
  --store <dir>      Forwarded to every run (real runs need the app store)
  --workdir <dir>    Outputs, reports and logs (default: <tmp>/koharu-bench-cross)
  --orders <which>   both (default, the real cross-over), 1 or 2 (single order,
                     stored in the workdir and merged with the other order
                     when you run it later in the same workdir)
  --pause <seconds>  Cooldown between the two orders so the second order's
                     first run is cold too (default: 600, 0 to skip)
  --plan             Run the checks and print the commands without translating
  -h, --help         Show this help

Anything after \`--\` is forwarded to every koharu-batch run (e.g. --llm gemma4-e4b-it).
Flags touching the calibration file are rejected: bench runs stay --no-calibration.`

function parseOptions(): Options {
  const { values, positionals } = parseArgs({
    args: process.argv.slice(2),
    options: {
      before: { type: 'string' },
      after: { type: 'string' },
      input: { type: 'string' },
      store: { type: 'string' },
      workdir: { type: 'string' },
    orders: { type: 'string', default: 'both' },
    pause: { type: 'string', default: '600' },
      plan: { type: 'boolean', default: false },
      help: { type: 'boolean', short: 'h', default: false },
    },
    strict: false,
    allowPositionals: true,
  })

  if (values.help) {
    console.log(USAGE)
    process.exit(0)
  }

  const missing = (['before', 'after', 'input'] as const).filter((key) => !values[key])
  if (missing.length > 0) {
    throw new Error(`missing required option(s): ${missing.map((key) => `--${key}`).join(', ')}\n\n${USAGE}`)
  }

  const orders = values.orders as Options['orders']
  if (!['both', '1', '2'].includes(orders)) {
    throw new Error(`--orders must be both, 1 or 2 (got ${String(values.orders)})`)
  }

  // The whole bench rests on --no-calibration: refuse anything that could
  // touch vram-calibration.toml. `--cpu` would bench the wrong device.
  const forbidden = ['calibration', '--cpu']
  const clash = positionals.find(
    (arg) => forbidden.some((flag) => arg.includes(flag)) && arg !== '--no-calibration',
  )
  if (clash) {
    throw new Error(`refusing to forward ${clash}: every bench run stays --no-calibration (AGENTS.md)`)
  }

  const pause = Number(values.pause ?? '600')
  if (!Number.isInteger(pause) || pause < 0) {
    throw new Error(`--pause must be a whole number of seconds (got ${String(values.pause)})`)
  }

  // parseArgs types every value as string | boolean; the string options are
  // declared together below, so narrow them once instead of at each use.
  const str = (key: 'before' | 'after' | 'input' | 'store' | 'workdir'): string | undefined =>
    typeof values[key] === 'string' ? values[key] : undefined
  const before = str('before')
  const after = str('after')
  const input = str('input')
  const store = str('store')
  const workdir = str('workdir')

  return {
    before: path.resolve(before!),
    after: path.resolve(after!),
    input: path.resolve(input!),
    store: store ? path.resolve(store) : undefined,
    workdir: workdir ? path.resolve(workdir) : path.join(tmpdir(), 'koharu-bench-cross'),
    orders,
    pause,
    plan: values.plan === true,
    extra: positionals,
    pin: [],
  }
}

function specs(options: Options): RunSpec[] {
  const binaries = {
    before: options.before,
    after: options.after,
  } as const
  // Execution orders: 1 = baseline first, 2 = candidate first (the cross-over).
  const sequences: Record<string, Array<'before' | 'after'>> = {
    '1': ['before', 'after'],
    '2': ['after', 'before'],
  }
  const selected = options.orders === 'both' ? ['1', '2'] : [options.orders]
  return selected.flatMap((order) =>
    sequences[order].map((binary, index) => ({
      tag: `o${order}-${binary}`,
      order: Number(order),
      position: index + 1,
      binary,
      bin: binaries[binary],
    })),
  )
}

function outputFor(options: Options, tag: string): string {
  const archive = /\.(cbz|zip)$/i.test(options.input)
  return path.join(options.workdir, archive ? `${tag}.cbz` : tag)
}

function runArgs(options: Options, spec: RunSpec): string[] {
  return [
    '--input',
    options.input,
    '--output',
    outputFor(options, spec.tag),
    '--report',
    path.join(options.workdir, spec.tag),
    '--no-calibration',
    ...(options.store ? ['--store', options.store] : []),
    ...options.pin,
    ...options.extra,
  ]
}

/// The `translation model: <id> <quantization>` line two runs must agree on.
function modelOf(text: string): string | undefined {
  const match = /^translation model: (\S+ \S+)/m.exec(text)
  return match?.[1]
}

/// Dry-run both binaries up front: comparing resolved models costs seconds
/// instead of discovering a mismatch after eighty minutes of translation.
/// Returns the model line plus the `--llm`/`--quantization` pin every run
/// gets — `auto` re-resolves by free VRAM on each start, and that budget
/// drifts during a session (observed: a mid-session switch of model).
function checkModels(options: Options): { model: string; pin: string[] } {
  const models = (['before', 'after'] as const).map((binary) => {
    const bin = binary === 'before' ? options.before : options.after
    const args = [
      '--input',
      options.input,
      '--output',
      path.join(options.workdir, `_model-check-${binary}`),
      '--no-calibration',
      '--dry-run',
      ...(options.store ? ['--store', options.store] : []),
      ...options.extra,
    ]
    const child = spawnSync(bin, args, { encoding: 'utf8', timeout: 120_000 })
    const model = modelOf(`${child.stdout ?? ''}\n${child.stderr ?? ''}`)
    if (child.status !== 0 || !model) {
      throw new Error(`dry run of the ${binary} binary failed (exit ${child.status}); cannot verify the model`)
    }
    return model
  })
  if (models[0] !== models[1]) {
    throw new Error(
      `binaries resolve different models (${models[0]} vs ${models[1]}) — the comparison would be invalid; pin one with -- --llm <id>`,
    )
  }
  // The dry runs point --output at placeholders: leave no trace of them.
  for (const binary of ['before', 'after']) {
    rmSync(path.join(options.workdir, `_model-check-${binary}`), { recursive: true, force: true })
  }
  // Pin what the dry runs agreed on; never duplicate a flag the user already
  // passed (clap rejects repeated options), and let theirs win.
  const given = (flag: string) => options.extra.some((arg) => arg === flag || arg.startsWith(`${flag}=`))
  const [id, quantization] = models[0].split(' ')
  const pin = [
    ...(given('--llm') ? [] : ['--llm', id]),
    ...(given('--quantization') ? [] : ['--quantization', quantization]),
  ]
  console.log(`model check: both binaries resolve ${models[0]} (pinned: ${pin.join(' ') || 'as passed'})`)
  return { model: models[0], pin }
}

/// Removes anything a previous invocation left under this run's tag (output,
/// report, state file, log) so a stale state can never turn runs into skips.
function resetTag(workdir: string, tag: string): void {
  for (const entry of readdirSync(workdir)) {
    if (entry.startsWith(tag)) {
      rmSync(path.join(workdir, entry), { recursive: true, force: true })
    }
  }
}

function runOne(options: Options, spec: RunSpec): Promise<RunResult> {
  const logPath = path.join(options.workdir, `${spec.tag}.log`)
  const log = createWriteStream(logPath, { flags: 'w' })
  const args = runArgs(options, spec)
  console.log(
    `[order ${spec.order}, ${spec.position === 1 ? '1st' : '2nd'} run] ${spec.binary}: ` +
      `${path.basename(spec.bin)} ${args.join(' ')}`,
  )
  const startedAt = Date.now()
  const child = spawn(spec.bin, args, { stdio: ['ignore', 'pipe', 'pipe'] })
  child.stdout.pipe(log)
  child.stderr.pipe(log)
  child.stdout.pipe(process.stdout)
  child.stderr.pipe(process.stderr)

  return new Promise((resolve, reject) => {
    child.on('error', (error) => {
      log.end()
      reject(error)
    })
    child.on('close', (code) => {
      log.end()
      const wall = (Date.now() - startedAt) / 1000
      const endedAt = new Date().toISOString().replace('T', ' ').slice(0, 19)
      if (code !== 0) {
        return reject(new Error(`${spec.tag} exited with ${code} — see ${logPath}`))
      }
      const report = parseReport(path.join(options.workdir, spec.tag))
      const model = modelOf(readFileSync(logPath, 'utf8'))
      if (!model) {
        return reject(new Error(`${spec.tag}: no resolved model line in ${logPath}`))
      }
      console.log(
        `[order ${spec.order}] ${spec.tag} done in ${wall.toFixed(1)}s ` +
          `(report ${report.reportSeconds.toFixed(1)}s, Σstages ${report.stageSeconds.toFixed(1)}s, ${endedAt})`,
      )
      resolve({
        spec,
        wall,
        endedAt,
        reportSeconds: report.reportSeconds,
        stageSeconds: report.stageSeconds,
        translated: report.translated,
        skipped: report.skipped,
        failed: report.failed,
        model,
      })
    })
  })
}

/// Wall-clock totals and per-stage sums from the `.md` report — the format
/// both bench binaries write (`--json` does not exist on the baseline yet).
function parseReport(base: string): Omit<RunResult, 'spec' | 'wall' | 'endedAt' | 'model'> {
  let text: string
  try {
    text = readFileSync(`${base}.md`, 'utf8')
  } catch (error) {
    throw new Error(`report ${base}.md missing after a successful run: ${error}`)
  }
  const total = /\| Durée totale \| ([\d.]+)s \|/.exec(text)
  const counts = /\| Pages \| (\d+) traduites?, (\d+) ignorées?, (\d+) en échec/.exec(text)
  if (!total || !counts) {
    throw new Error(`cannot parse the header of ${base}.md`)
  }
  let stageSeconds = 0
  for (const row of text.split('\n')) {
    if (!/^\| \d+ \|/.test(row)) continue
    const cells = row.split('|')
    const detail = cells[cells.length - 2] ?? ''
    for (const match of detail.matchAll(/([a-z][a-z-]*) ([\d.]+)s/g)) {
      stageSeconds += Number(match[2])
    }
  }
  if (stageSeconds <= 0) {
    throw new Error(`no stage durations parsed from ${base}.md`)
  }
  return {
    reportSeconds: Number(total[1]),
    stageSeconds: Math.round(stageSeconds * 10) / 10,
    translated: Number(counts[1]),
    skipped: Number(counts[2]),
    failed: Number(counts[3]),
  }
}

function checkResult(reference: RunResult, result: RunResult): void {
  if (result.model !== reference.model) {
    throw new Error(
      `${result.spec.tag} resolved ${result.model} but ${reference.spec.tag} resolved ${reference.model} — ` +
        'the runs are not comparable',
    )
  }
  const pages = `${reference.translated}/${reference.skipped}/${reference.failed}`
  const own = `${result.translated}/${result.skipped}/${result.failed}`
  if (own !== pages) {
    throw new Error(
      `${result.spec.tag} reports ${own} pages (translated/skipped/failed) but ${reference.spec.tag} reported ${pages} — ` +
        'a stale state or a changed input would skew the residuals',
    )
  }
}

/// Persisted results of earlier invocations of this workdir, so two single-ordered
/// sessions (`--orders 1`, then `--orders 2`) merge into one cross-over summary.
interface StoredRuns {
  input: string
  before: string
  after: string
  model: string
  results: RunResult[]
}

function runsPath(options: Options): string {
  return path.join(options.workdir, 'runs.json')
}

/// Stored runs from a *different* experiment (other input, binaries or model)
/// must never blend into this one — they are dropped with a note instead.
function loadRuns(options: Options, model: string): RunResult[] {
  let text: string
  try {
    text = readFileSync(runsPath(options), 'utf8')
  } catch {
    return []
  }
  let stored: StoredRuns
  try {
    stored = JSON.parse(text) as StoredRuns
  } catch {
    console.log(`ignoring unreadable ${runsPath(options)}`)
    return []
  }
  if (
    stored.input !== options.input ||
    stored.before !== options.before ||
    stored.after !== options.after ||
    stored.model !== model
  ) {
    console.log(`ignoring ${runsPath(options)}: it records a different input, binary pair or model`)
    return []
  }
  // Tags this invocation is about to re-run are not worth keeping.
  const rerun = new Set(specs(options).map((spec) => spec.tag))
  return stored.results.filter((result) => !rerun.has(result.spec.tag))
}

function saveRuns(options: Options, model: string, results: RunResult[]): void {
  const stored: StoredRuns = {
    input: options.input,
    before: options.before,
    after: options.after,
    model,
    results,
  }
  writeFileSync(runsPath(options), `${JSON.stringify(stored, null, 2)}\n`)
}

function summarise(options: Options, model: string, results: RunResult[]): string {
  const lines: string[] = []
  lines.push('# Cross-over bench — koharu-batch')
  lines.push('')
  lines.push(`- input: \`${options.input}\``)
  lines.push(`- before: \`${options.before}\``)
  lines.push(`- after: \`${options.after}\``)
  lines.push(`- model: ${model}, store: \`${options.store ?? '(binary default)'}\`, \`--no-calibration\``)
  lines.push(`- finished: ${new Date().toISOString().replace('T', ' ').slice(0, 19)}`)
  lines.push('')
  const ordered = [...results].sort((a, b) => a.spec.order - b.spec.order || a.spec.position - b.spec.position)
  lines.push('| order | position | binary | wall | report | Σstages | residual |')
  lines.push('|---:|---|---|---:|---:|---:|---:|')
  for (const r of ordered) {
    const residual = r.wall - r.stageSeconds
    const position = r.spec.position === 1 ? '1st (cold start)' : '2nd (warm)'
    lines.push(
      `| ${r.spec.order} | ${position} | ${r.spec.binary} | ${r.wall.toFixed(1)}s | ` +
        `${r.reportSeconds.toFixed(1)}s | ${r.stageSeconds.toFixed(1)}s | **${residual.toFixed(1)}s** |`,
    )
  }
  lines.push('')

  const orders = new Set(ordered.map((r) => r.spec.order))
  const partial = orders.size < 2
  if (partial) {
    const missing = orders.has(1) ? '2' : '1'
    lines.push(
      `> Single order recorded: GPU warm-up is NOT cancelled — rerun with ` +
        `\`--orders ${missing}\` in the same workdir (same input and binaries) ` +
        'to merge both orders into the neutralised comparison.',
    )
    lines.push('')
  } else {
    // Per slot, not per execution: order 2 mirrors order 1, so each binary
    // sat once in the cold slot and once in the warm slot.
    const slot = (binary: 'before' | 'after', position: number) =>
      ordered.find((r) => r.spec.binary === binary && r.spec.position === position)!
    const residual = (binary: 'before' | 'after', position: number) => {
      const run = slot(binary, position)
      return run.wall - run.stageSeconds
    }
    const mean = (values: number[]) => values.reduce((sum, value) => sum + value, 0) / values.length
    const before = [residual('before', 1), residual('before', 2)]
    const after = [residual('after', 1), residual('after', 2)]
    const meanBefore = mean(before)
    const meanAfter = mean(after)
    const pages = results[0].translated
    const gain = meanBefore - meanAfter
    const positionEffect =
      mean(ordered.filter((r) => r.spec.position === 1).map((r) => r.stageSeconds)) -
      mean(ordered.filter((r) => r.spec.position === 2).map((r) => r.stageSeconds))
    lines.push('| residual (`wall − Σstages`) | cold slot (1st) | warm slot (2nd) | mean | per page |')
    lines.push('|---|---:|---:|---:|---:|')
    lines.push(
      `| before | ${before[0].toFixed(1)}s | ${before[1].toFixed(1)}s | ${meanBefore.toFixed(1)}s | ` +
        `${(meanBefore / pages).toFixed(2)}s |`,
    )
    lines.push(
      `| after | ${after[0].toFixed(1)}s | ${after[1].toFixed(1)}s | ${meanAfter.toFixed(1)}s | ` +
        `${(meanAfter / pages).toFixed(2)}s |`,
    )
    lines.push('')
    lines.push(
      `- **order-neutralised gain: ${gain >= 0 ? '-' : '+'}${Math.abs(gain).toFixed(1)}s ` +
        `(${(Math.abs(gain) / pages).toFixed(2)}s/page)** across ${pages} pages`,
    )
    lines.push(
      `- position effect on Σstages (warm-up): ${positionEffect >= 0 ? '-' : '+'}${Math.abs(positionEffect).toFixed(1)}s ` +
        'between 1st and 2nd runs',
    )
    lines.push('- wall times are process-spawn to exit; residuals ignore stage generation variance by construction')
    lines.push('')
  }
  return lines.join('\n')
}

async function main(): Promise<void> {
  const options = parseOptions()
  for (const binary of [options.before, options.after]) {
    if (!statSync(binary, { throwIfNoEntry: false })) {
      throw new Error(`binary not found: ${binary}`)
    }
  }
  if (!statSync(options.input, { throwIfNoEntry: false })) {
    throw new Error(`input not found: ${options.input}`)
  }
  mkdirSync(options.workdir, { recursive: true })

  const runs = specs(options)
  const { model, pin } = checkModels(options)
  options.pin = pin
  console.log(`workdir: ${options.workdir} (${runs.length} run(s): ${runs.map((spec) => spec.tag).join(', ')})`)

  if (options.plan) {
    for (const spec of runs) {
      console.log(`plan ${spec.tag}: ${spec.bin} ${runArgs(options, spec).join(' ')}`)
    }
    return
  }

  // Runs kept from an earlier invocation of the same experiment (e.g. the
  // other order, run yesterday) merge with this session's results.
  const results = loadRuns(options, model)
  if (results.length > 0) {
    console.log(`resuming ${results.length} stored run(s) from ${runsPath(options)}`)
  }
  for (const [index, spec] of runs.entries()) {
    if (index > 0 && options.pause > 0 && spec.position === 1) {
      console.log(`pausing ${options.pause}s between orders…`)
      await new Promise((resolve) => setTimeout(resolve, options.pause * 1000))
    }
    resetTag(options.workdir, spec.tag)
    const result = await runOne(options, spec)
    if (results.length > 0) {
      checkResult(results[0], result)
    }
    results.push(result)
    // Persist after every run: an interrupted session keeps what it finished.
    saveRuns(options, model, results)
  }

  const summary = summarise(options, model, results)
  const summaryPath = path.join(options.workdir, 'summary.md')
  writeFileSync(summaryPath, summary)
  console.log(`\n${summary}\nsummary written to ${summaryPath}`)
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : error)
  process.exit(1)
})
