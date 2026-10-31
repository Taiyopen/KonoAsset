// Builds a signed installer and publishes it to this fork's GitHub Releases,
// where the app's updater (tauri.conf.json > plugins.updater.endpoints) looks for latest.json.
//
// Usage:
//   node scripts/release-fork.mjs              build, sign and publish
//   node scripts/release-fork.mjs --build-only build and sign only (nothing is uploaded)
//
// The private key is read from KONOASSET_SIGNING_KEY_PATH, or signing/konoasset-fork.key.
// Set TAURI_SIGNING_PRIVATE_KEY_PASSWORD if the key has a password.

import { execFileSync, execSync } from 'child_process'
import fs from 'fs'
import os from 'os'
import path from 'path'

const REPO = 'Taiyopen/KonoAsset'
const buildOnly = process.argv.includes('--build-only')

const run = (command) => execSync(command, { stdio: 'inherit' })
const output = (file, args) =>
  execFileSync(file, args, { encoding: 'utf8' }).trim()

const version = JSON.parse(
  fs.readFileSync('src-backend/tauri/tauri.conf.json', 'utf8'),
).version
const tag = `konoasset-v${version}`

const changelog = JSON.parse(fs.readFileSync('changelog.json', 'utf8'))
if (changelog[0]?.version !== version) {
  throw new Error(
    `changelog.json does not start with ${version}; add its entry first (the app shows it when updating)`,
  )
}

const notes = (() => {
  const lines = fs.readFileSync('CHANGELOG.md', 'utf8').split(/\r?\n/)
  const start = lines.indexOf(`## ${version}`)
  if (start === -1) {
    throw new Error(`CHANGELOG.md has no "## ${version}" section`)
  }
  const end = lines.findIndex((line, i) => i > start && line.startsWith('## '))
  return lines
    .slice(start + 1, end === -1 ? undefined : end)
    .join('\n')
    .trim()
})()

if (!buildOnly) {
  if (output('git', ['status', '--porcelain', '--untracked-files=no']) !== '') {
    throw new Error('Commit your changes before publishing')
  }
  run('git fetch -q origin main')
  if (
    output('git', ['rev-parse', 'HEAD']) !==
    output('git', ['rev-parse', 'origin/main'])
  ) {
    throw new Error(
      'Push main to origin before publishing (the release is tagged on origin/main)',
    )
  }
  const existing = output('gh', [
    'release',
    'list',
    '--repo',
    REPO,
    '--json',
    'tagName',
    '--jq',
    '.[].tagName',
  ])
  if (existing.split('\n').includes(tag)) {
    throw new Error(`Release ${tag} already exists; bump the version first`)
  }
}

const keyPath =
  process.env.KONOASSET_SIGNING_KEY_PATH ??
  path.join('signing', 'konoasset-fork.key')
process.env.TAURI_SIGNING_PRIVATE_KEY = fs.readFileSync(keyPath, 'utf8')
process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= ''

run('pnpm tauri build')

const bundleDir = 'src-backend/target/release/bundle/nsis'
const installerName = `KonoAsset_${version}_x64-setup.exe`
const installer = path.join(bundleDir, installerName)
const signature = fs.readFileSync(`${installer}.sig`, 'utf8').trim()

const latestJson = path.join(bundleDir, 'latest.json')
fs.writeFileSync(
  latestJson,
  JSON.stringify(
    {
      version,
      notes,
      pub_date: new Date().toISOString(),
      platforms: {
        'windows-x86_64': {
          signature,
          url: `https://github.com/${REPO}/releases/download/${tag}/${installerName}`,
        },
      },
    },
    null,
    2,
  ),
)

console.log(`\nBuilt ${installer}`)

if (buildOnly) {
  console.log('--build-only: nothing was uploaded')
  process.exit(0)
}

const notesFile = path.join(os.tmpdir(), `konoasset-${version}-notes.md`)
fs.writeFileSync(notesFile, notes)

execFileSync(
  'gh',
  [
    'release',
    'create',
    tag,
    installer,
    `${installer}.sig`,
    latestJson,
    '--repo',
    REPO,
    '--target',
    'main',
    '--title',
    `KonoAsset v${version}`,
    '--notes-file',
    notesFile,
  ],
  { stdio: 'inherit' },
)

console.log(`\nPublished https://github.com/${REPO}/releases/tag/${tag}`)
