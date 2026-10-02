// Extracts the prune smoke step from the workflow, substitutes the GitHub
// expressions like the runner would, and writes a runnable .ps1:
//   bun scripts/test-prune-step.ts <out.ps1>
import { createRequire } from 'node:module'
import { readFileSync, writeFileSync } from 'node:fs'

const require = createRequire(import.meta.url)
const yaml = require('js-yaml')

// Typed explicitly: with TypeScript 7 (pinned) the callback of `find` on the
// untyped `yaml.load` result gets no contextual parameter type, and `s` is
// reported as an implicit any.
type WorkflowStep = { name?: string; run?: string }
const doc = yaml.load(readFileSync('.github/workflows/koharu-batch.yml', 'utf8')) as {
  jobs: { windows: { steps: WorkflowStep[] } }
}
const step = doc.jobs.windows.steps.find((s) => s.name && s.name.includes('prune'))
if (!step?.run) throw new Error('no prune step found in the workflow')
const [out] = process.argv.slice(2)
if (!out) throw new Error('usage: bun scripts/test-prune-step.ts <out.ps1>')
const runnerTemp = process.env.RUNNER_TEMP ?? process.env.TEMP
if (!runnerTemp) throw new Error('RUNNER_TEMP or TEMP must be set')

const script = step.run
  .replaceAll('${{ runner.temp }}', runnerTemp)
  .replaceAll('${{ env.LLAMA_RELEASE }}', process.env.LLAMA_RELEASE ?? 'b10903')

writeFileSync(out, script)
console.log(`wrote ${out} (${script.split('\n').length} lines)`)
