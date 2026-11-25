#!/usr/bin/env bun

import { chmodSync, copyFileSync, mkdirSync } from 'node:fs'
import path from 'node:path'

const root = path.dirname(import.meta.dir)

const rustc = Bun.spawnSync({
  cmd: ['rustc', '-vV'],
  cwd: root,
  stdout: 'pipe',
  stderr: 'inherit',
})
const host = new TextDecoder().decode(rustc.stdout).match(/^host: (.+)$/m)?.[1]
if (rustc.exitCode !== 0 || !host) {
  throw new Error('rustc -vV did not report the host triple')
}

const build = Bun.spawnSync({
  cmd: ['cargo', 'build', '--release', '-p', 'koharu-batch'],
  cwd: root,
  stdout: 'inherit',
  stderr: 'inherit',
})
if (build.exitCode !== 0) process.exit(build.exitCode ?? 1)

const extension = process.platform === 'win32' ? '.exe' : ''
const source = path.join(root, 'target', 'release', `koharu-batch${extension}`)
const directory = path.join(root, 'crates', 'koharu', 'binaries')
const destination = path.join(directory, `koharu-batch-${host}${extension}`)

mkdirSync(directory, { recursive: true })
copyFileSync(source, destination)
if (process.platform !== 'win32') chmodSync(destination, 0o755)

console.log(`sidecar ${path.relative(root, destination)}`)
