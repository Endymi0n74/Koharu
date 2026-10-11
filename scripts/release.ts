#!/usr/bin/env bun

import { exec as execCallback } from 'node:child_process'
import { readFile, writeFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import path from 'node:path'

const exec = promisify(execCallback)
const root = path.resolve(__dirname, '..')
const execOpts = { cwd: root, maxBuffer: 10 * 1024 * 1024 }

async function main() {
  let bumpedVersion = process.argv[2]?.trim()

  if (bumpedVersion) {
    console.log(`Using provided version: ${bumpedVersion}`)
  } else {
    console.log('Calculating bumped version with git-cliff...')
    bumpedVersion = (
      await exec('bun git-cliff --offline --unreleased --bumped-version', execOpts)
    ).stdout.trim()
  }

  if (!bumpedVersion) {
    throw new Error('git-cliff did not return a bumped version')
  }

  // git-cliff echoes the fork's tag prefix back (`v0.85.0`), but
  // [workspace.package] wants the bare semver while the tag and the release
  // commit keep the `v` — release.yml only triggers on `v*` tags.
  const version = bumpedVersion.replace(/^v/, '')
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`"${bumpedVersion}" is not an x.y.z version`)
  }
  const tag = `v${version}`

  console.log(`Release ${tag} (workspace version ${version})`)

  const cargoTomlPath = path.join(root, 'Cargo.toml')
  const cargoToml = await readFile(cargoTomlPath, 'utf8')
  const versionPattern =
    /(\[workspace\.package\][\s\S]*?version\s*=\s*")([^"]+)(")/

  if (!versionPattern.test(cargoToml)) {
    throw new Error('Could not find [workspace.package] version in Cargo.toml')
  }

  const updatedCargoToml = cargoToml.replace(versionPattern, `$1${version}$3`)
  await writeFile(cargoTomlPath, updatedCargoToml)
  console.log('Updated Cargo.toml version')

  await exec('cargo metadata --format-version 1', execOpts)
  console.log('Updated Cargo.lock')

  await exec('git add Cargo.toml Cargo.lock', execOpts)
  await exec(`git commit -m "chore(release): ${tag}"`, execOpts)
  console.log('Created release commit')

  await exec(`git tag ${tag}`, execOpts)
  console.log('Created git tag')

  await exec(`bun git-cliff --offline -o CHANGELOG.md`, execOpts)
  console.log('Updated CHANGELOG.md')

  await exec('git add CHANGELOG.md', execOpts)
  await exec(`git commit --amend --no-edit`, execOpts)
  console.log('Amended release commit with updated CHANGELOG.md')

  await exec(`git tag -f ${tag}`, execOpts)
  console.log('Updated git tag to include CHANGELOG.md')

  console.log(`Release commit and tag ${tag} created.`)
}

main().catch((error) => {
  console.error(error)
  process.exit(1)
})
