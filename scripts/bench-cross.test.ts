import { describe, expect, it } from 'bun:test'

import path from 'node:path'

import { parseOptions, specs, summarise } from './bench-cross'
import type { Options, RunResult } from './bench-cross'

const baseArgs = ['--before', '/bin/before', '--after', '/bin/after', '--input', '/data/pages']
// parseOptions resolves paths — assertions must compare resolved forms to
// stay portable: POSIX roots become drive-qualified paths on Windows.
const before = path.resolve('/bin/before')
const after = path.resolve('/bin/after')

function options(argv: string[] = []): Options {
  return parseOptions([...baseArgs, ...argv])
}

function result(
  order: 1 | 2,
  position: 1 | 2,
  binary: 'before' | 'after',
  wall: number,
  stageSeconds: number,
  translated = 50,
): RunResult {
  return {
    spec: { tag: `o${order}-${binary}`, order, position, binary, bin: path.resolve(`/bin/${binary}`) },
    wall,
    endedAt: '2026-09-26 12:00:00',
    reportSeconds: wall - 1,
    stageSeconds,
    translated,
    skipped: 2,
    failed: 0,
    model: 'gemma4-e4b-it q4_1',
  }
}

describe('parseOptions', () => {
  it('requires --before, --after and --input', () => {
    expect(() => parseOptions([])).toThrow('missing required option(s): --before, --after, --input')
    expect(() => parseOptions(['--before', '/bin/before'])).toThrow(
      'missing required option(s): --after, --input',
    )
  })

  it('resolves paths and applies the documented defaults', () => {
    const opts = options()
    expect(opts.before).toBe(before)
    expect(opts.after).toBe(after)
    expect(opts.input).toBe(path.resolve('/data/pages'))
    expect(opts.store).toBeUndefined()
    expect(opts.workdir).toContain('koharu-bench-cross')
    expect(opts.orders).toBe('both')
    expect(opts.pause).toBe(600)
    expect(opts.plan).toBe(false)
    expect(opts.extra).toEqual([])
    expect(opts.pin).toEqual([])
  })

  it('rejects an unknown --orders value', () => {
    expect(() => options(['--orders', '3'])).toThrow('--orders must be both, 1 or 2 (got 3)')
    expect(() => options(['--orders', 'BOTH'])).toThrow('--orders must be both, 1 or 2')
    expect(options(['--orders', '1']).orders).toBe('1')
    expect(options(['--orders', '2']).orders).toBe('2')
  })

  it('rejects a negative or fractional --pause', () => {
    expect(() => options(['--pause', '-1'])).toThrow('--pause must be a whole number of seconds')
    expect(() => options(['--pause', '1.5'])).toThrow('--pause must be a whole number of seconds')
    expect(() => options(['--pause', 'banana'])).toThrow('--pause must be a whole number of seconds')
    expect(options(['--pause', '0']).pause).toBe(0)
  })

  it('refuses calibration and --cpu flags whether before or after `--`', () => {
    expect(() => options(['--', '--calibration'])).toThrow(
      'refusing to forward --calibration: every bench run stays --no-calibration',
    )
    expect(() => options(['--', '--cpu'])).toThrow('refusing to forward --cpu')
    // Before `--` they land in `values` (strict: false) — still refused.
    expect(() => options(['--calibration'])).toThrow('refusing to forward --calibration')
    expect(() => options(['--calibration=false'])).toThrow('refusing to forward --calibration')
    expect(() => options(['--cpu'])).toThrow('refusing to forward --cpu')
    // The legitimate flag stays allowed.
    expect(() => options(['--', '--no-calibration'])).not.toThrow()
  })

  it('forwards everything after `--` as extra args', () => {
    expect(options(['--', '--llm', 'gemma4-e4b-it', '--quantization', 'q4_1']).extra).toEqual([
      '--llm',
      'gemma4-e4b-it',
      '--quantization',
      'q4_1',
    ])
    expect(options(['--plan']).plan).toBe(true)
    expect(options(['--store', '/data/store']).store).toBe(path.resolve('/data/store'))
  })
})

describe('specs — cross-over execution orders', () => {
  it('runs both orders so each binary occupies each slot exactly once', () => {
    const runs = specs(options())
    expect(runs.map((spec) => spec.tag)).toEqual(['o1-before', 'o1-after', 'o2-after', 'o2-before'])
    expect(runs.map((spec) => spec.order)).toEqual([1, 1, 2, 2])
    expect(runs.map((spec) => spec.position)).toEqual([1, 2, 1, 2])
    expect(runs.map((spec) => spec.binary)).toEqual(['before', 'after', 'after', 'before'])
    // The cross-over invariant: each slot (cold and warm) holds each binary once.
    expect(
      runs
        .filter((spec) => spec.position === 1)
        .map((spec) => spec.binary)
        .sort(),
    ).toEqual(['after', 'before'])
    expect(
      runs
        .filter((spec) => spec.position === 2)
        .map((spec) => spec.binary)
        .sort(),
    ).toEqual(['after', 'before'])
  })

  it('binds each spec to its binary path', () => {
    const runs = specs(options())
    expect(runs.find((spec) => spec.tag === 'o1-before')?.bin).toBe(before)
    expect(runs.find((spec) => spec.tag === 'o2-after')?.bin).toBe(after)
  })

  it('runs a single order when asked, in the right sequence', () => {
    const order1 = specs(options(['--orders', '1']))
    expect(order1.map((spec) => spec.tag)).toEqual(['o1-before', 'o1-after'])
    expect(order1.every((spec) => spec.order === 1)).toBe(true)

    const order2 = specs(options(['--orders', '2']))
    expect(order2.map((spec) => spec.tag)).toEqual(['o2-after', 'o2-before'])
    expect(order2.every((spec) => spec.order === 2)).toBe(true)
  })
})

describe('summarise — cross-over summary', () => {
  // Residuals (wall − Σstages): before 20/10 → mean 15.0,
  // after 15/10 → mean 12.5, pages = 50.
  const full = (): RunResult[] => [
    result(1, 1, 'before', 100, 80), // residual 20, cold
    result(1, 2, 'after', 70, 60), // residual 10, warm
    result(2, 1, 'after', 85, 70), // residual 15, cold
    result(2, 2, 'before', 75, 65), // residual 10, warm
  ]

  it('reports the order-neutralised gain and per-page cost when both orders are present', () => {
    const summary = summarise(options(), 'gemma4-e4b-it q4_1', full())
    // gain = meanBefore − meanAfter = 15.0 − 12.5 = 2.5s over 50 pages.
    expect(summary).toContain(
      'order-neutralised gain: -2.5s (0.05s/page)** across 50 pages',
    )
    expect(summary).toContain('| before | 20.0s | 10.0s | 15.0s | 0.30s |')
    expect(summary).toContain('| after | 15.0s | 10.0s | 12.5s | 0.25s |')
    // mean Σstages position 1 (80, 70) − position 2 (60, 65) = 12.5s.
    expect(summary).toContain('position effect on Σstages (warm-up): -12.5s')
    expect(summary).toContain('- model: gemma4-e4b-it q4_1')
    expect(summary).toContain('`--no-calibration`')
    expect(summary).not.toContain('Single order recorded')
  })

  it('signs a real gain when the after binary is lighter', () => {
    const results = [
      result(1, 1, 'before', 100, 80), // residual 20
      result(1, 2, 'after', 65, 60), // residual 5
      result(2, 1, 'after', 70, 65), // residual 5
      result(2, 2, 'before', 85, 75), // residual 10
    ]
    const summary = summarise(options(), 'gemma4-e4b-it q4_1', results)
    // before mean 15, after mean 5 → gain 10.0s over 50 pages.
    expect(summary).toContain('order-neutralised gain: -10.0s (0.20s/page)** across 50 pages')
  })

  it('warns that a single order leaves GPU warm-up uncancelled', () => {
    const only1 = summarise(options(['--orders', '1']), 'gemma4-e4b-it q4_1', full().slice(0, 2))
    expect(only1).toContain('Single order recorded: GPU warm-up is NOT cancelled')
    expect(only1).toContain('--orders 2')
    expect(only1).not.toContain('order-neutralised gain')

    const only2 = summarise(options(['--orders', '2']), 'gemma4-e4b-it q4_1', full().slice(2))
    expect(only2).toContain('--orders 1')
  })

  it('labels the cold and warm slots per execution order', () => {
    const summary = summarise(options(), 'gemma4-e4b-it q4_1', full())
    expect(summary).toContain('| 1 | 1st (cold start) | before | 100.0s | 99.0s | 80.0s | **20.0s** |')
    expect(summary).toContain('| 2 | 1st (cold start) | after | 85.0s | 84.0s | 70.0s | **15.0s** |')
    expect(summary).toContain('| 1 | 2nd (warm) | after | 70.0s | 69.0s | 60.0s | **10.0s** |')
    expect(summary).toContain('| 2 | 2nd (warm) | before | 75.0s | 74.0s | 65.0s | **10.0s** |')
  })

  it('sorts rows by order then position regardless of input order', () => {
    const source = full()
    const shuffled = [source[2], source[0], source[3], source[1]]
    const summary = summarise(options(), 'gemma4-e4b-it q4_1', shuffled)
    const rows = summary
      .split('\n')
      .filter((line) => /^\| [12] \|/.test(line))
      .map((line) => /^\| (\d) \| ([^|]+)/.exec(line)?.slice(1, 3).join(' ').trim())
    expect(rows).toEqual([
      '1 1st (cold start)',
      '1 2nd (warm)',
      '2 1st (cold start)',
      '2 2nd (warm)',
    ])
  })
})
