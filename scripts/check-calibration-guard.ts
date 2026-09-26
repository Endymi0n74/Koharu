#!/usr/bin/env bun
// CI guard for the anti-calibration refusal (AGENTS.md: bench runs stay
// --no-calibration; vram-calibration.toml must never be touched).
//
// The `--help` smoke cannot see a guard that was silently disabled:
// parseOptions answers `--help` before the guard is ever evaluated. This
// script closes that hole with two probes of the same command (full required
// options plus a calibration flag, never `--help`):
//
//   1. the intact script must refuse, with the guard's own message;
//   2. a mutant with `if (clash)` disabled must get past the guard and die on
//      the (nonexistent) binaries instead — proving the refusal observed in
//      (1) comes from the guard, and that this probe can tell the two apart.
//
// If someone disables the guard for real, probe 1 fails and CI goes red.

import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

const scriptPath = path.join(import.meta.dir, 'bench-cross.ts')
const GUARD = 'if (clash) {'
const DISABLED = 'if (false && clash) {'
const REFUSAL = 'refusing to forward'
const PAST_GUARD = 'binary not found'

interface Probe {
  status: number | null
  output: string
}

function probe(target: string, flag: string): Probe {
  // Every required option, so the run reaches the guard itself. The binaries
  // never exist, but flags resolve long before they are stat'ed.
  const noop = path.join(tmpdir(), `koharu-guard-probe-${flag.slice(2)}`)
  const run = spawnSync(
    process.execPath,
    [target, '--before', noop, '--after', noop, '--input', noop, flag],
    { encoding: 'utf8' },
  )
  return { status: run.status, output: `${run.stdout}\n${run.stderr}` }
}

const source = readFileSync(scriptPath, 'utf8')
const failures: string[] = []

// Probe the intact script first: a missing refusal means the guard itself is
// dead, whatever its source looks like.
for (const flag of ['--calibration', '--cpu']) {
  const intact = probe(scriptPath, flag)
  if (intact.status === 0 || !intact.output.includes(`${REFUSAL} ${flag}`)) {
    failures.push(
      `intact script no longer refuses ${flag} (exit ${intact.status}) — the guard is dead:\n${intact.output.trim()}`,
    )
  }
}

const guards = source.split(GUARD).length - 1
if (failures.length > 0) {
  console.error(`check-calibration-guard failed:\n\n${failures.join('\n\n')}`)
  process.exit(1)
}

if (guards !== 1) {
  console.error(
    `check-calibration-guard: expected exactly one \`${GUARD}\` in bench-cross.ts, found ${guards} — update the mutant`,
  )
  process.exit(1)
}

const dir = mkdtempSync(path.join(tmpdir(), 'koharu-guard-mutant-'))
try {
  const mutant = path.join(dir, 'bench-cross.ts')
  writeFileSync(mutant, source.replace(GUARD, DISABLED))

  for (const flag of ['--calibration', '--cpu']) {
    const mutated = probe(mutant, flag)
    if (mutated.output.includes(REFUSAL)) {
      failures.push(
        `probe cannot tell a disabled guard apart: the mutant still refuses ${flag}:\n${mutated.output.trim()}`,
      )
    } else if (!mutated.output.includes(PAST_GUARD)) {
      failures.push(
        `the mutant never reached main() (${PAST_GUARD} missing) — it likely failed to load, so this probe proves nothing:\n${mutated.output.trim()}`,
      )
    }
  }
} finally {
  rmSync(dir, { recursive: true, force: true })
}

if (failures.length > 0) {
  console.error(`check-calibration-guard failed:\n\n${failures.join('\n\n')}`)
  process.exit(1)
}
console.log('check-calibration-guard: intact script refuses --calibration/--cpu, disabled guard is detected')
