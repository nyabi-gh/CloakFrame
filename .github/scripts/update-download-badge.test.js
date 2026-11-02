// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2026 Nyabi (nyabi-gh)

const assert = require('node:assert/strict')
const test = require('node:test')

const updateDownloadBadge = require('./update-download-badge.js')
const {
  badgePayload,
  countsPayload,
  distributionDownloadTotal,
  isDistributionAsset,
} = updateDownloadBadge

test('includes user downloads and full updater packages', () =>
{
  for (const name of [
    'CloakFrame-1.12.0-macOS-arm64.dmg',
    'CloakFrame-1.12.0-Windows-x64-Setup.exe',
    'CloakFrame-1.12.0-Linux-x86_64.AppImage',
    'CloakFrame-macOS-arm64.dmg',
    'CloakFrame-Windows-x64-Setup.exe',
    'CloakFrame-1.10.2-windows-x64.zip',
    'CloakFrame-Preview-2.0.0-preview.1-macOS-arm64.zip',
    'CloakFrame-Linux-x86_64.AppImage',
    'CloakFrame-1.10.2-x86_64.AppImage',
    'CloakFrame-1.11.0-full.nupkg',
    'CloakFrame-1.11.0-linux-full.nupkg',
  ])
  {
    assert.equal(isDistributionAsset(name), true, name)
  }
})

test('keeps counting distributions released before the CloakFrame rename', () =>
{
  for (const name of [
    'Redactly-1.0.0-arm64.dmg',
    'Redactly-win-Setup.exe',
    'Redactly-1.10.1-windows-x64.zip',
    'Redactly.AppImage',
    'Redactly-1.0.0-full.nupkg',
    'FaceVeil-1.3.0-windows-x64.zip',
  ])
  {
    assert.equal(isDistributionAsset(name), true, name)
  }
})

test('excludes updater metadata, delta packages, and checksums', () =>
{
  for (const name of [
    'appcast.xml',
    'releases.win.json',
    'releases.linux.json',
    'CloakFrame-1.11.0-delta.nupkg',
    'SHA256SUMS',
  ])
  {
    assert.equal(isDistributionAsset(name), false, name)
  }
})

test('sums public distribution assets only', () =>
{
  const releases = [
    {
      draft: false,
      assets: [
        { name: 'appcast.xml', download_count: 118 },
        { name: 'CloakFrame-1.11.0-arm64.dmg', download_count: 15 },
        { name: 'CloakFrame-win-Setup.exe', download_count: 9 },
        { name: 'CloakFrame-1.11.0-full.nupkg', download_count: 6 },
      ],
    },
    {
      draft: true,
      assets: [
        { name: 'CloakFrame.AppImage', download_count: 20 },
      ],
    },
  ]
  assert.equal(distributionDownloadTotal(releases), 30)
  assert.equal(JSON.parse(badgePayload(30)).message, '30')
})

test('lists each counted file by release, leaving out drafts and updater metadata', () =>
{
  const releases = [
    {
      tag_name: 'v1.12.1',
      draft: false,
      assets: [
        { name: 'CloakFrame-1.12.1-Windows-x64-Setup.exe', download_count: 17 },
        { name: 'CloakFrame-1.12.1-Linux-x86_64.AppImage', download_count: 35 },
        { name: 'releases.win.json', download_count: 74 },
      ],
    },
    {
      tag_name: 'v2.0.0-preview.2',
      draft: true,
      assets: [{ name: 'CloakFrame-2.0.0-preview.2-Windows-x64-Setup.exe', download_count: 0 }],
    },
  ]
  assert.deepEqual(JSON.parse(countsPayload(releases)), [
    { release: 'v1.12.1', name: 'CloakFrame-1.12.1-Linux-x86_64.AppImage', downloads: 35 },
    { release: 'v1.12.1', name: 'CloakFrame-1.12.1-Windows-x64-Setup.exe', downloads: 17 },
  ])
})

function fakeGitHub(files)
{
  const listed = []
  const written = []
  const github = {
    paginate: async (_method, { owner, repo }) =>
    {
      listed.push(`${owner}/${repo}`)
      return [
        {
          tag_name: 'v2.0.0',
          draft: false,
          assets: [{ name: 'CloakFrame-2.0.0-Windows-x64-Setup.exe', download_count: 4 }],
        },
      ]
    },
    rest: {
      repos: {
        listReleases: {},
        getContent: async ({ path }) =>
        {
          if (!(path in files))
          {
            throw Object.assign(new Error('Not Found'), { status: 404 })
          }
          return { data: { content: Buffer.from(files[path]).toString('base64'), sha: path } }
        },
        createOrUpdateFileContents: async request => written.push(request),
      },
      git: {
        getRef: async () => ({}),
      },
    },
  }
  return { github, listed, written }
}

const context = { repo: { owner: 'nyabi-gh', repo: 'CloakFrame' }, sha: 'abc' }
const core = { info: () => {} }

test('writes the badge and the counts by file from this repository\'s releases', async () =>
{
  const { github, listed, written } = fakeGitHub({})

  await updateDownloadBadge({ github, context, core })

  assert.deepEqual(listed, ['nyabi-gh/CloakFrame'])
  const read = path => JSON.parse(Buffer.from(
    written.find(request => request.path === path).content, 'base64').toString('utf8'))
  assert.equal(written.length, 2)
  assert.equal(read('downloads.json').message, '4')
  assert.deepEqual(read('downloads-by-file.json'), [
    { release: 'v2.0.0', name: 'CloakFrame-2.0.0-Windows-x64-Setup.exe', downloads: 4 },
  ])
})

test('writes nothing when the counts have not changed', async () =>
{
  const { github: first, written: before } = fakeGitHub({})
  await updateDownloadBadge({ github: first, context, core })
  const files = Object.fromEntries(before.map(request =>
    [request.path, Buffer.from(request.content, 'base64').toString('utf8')]))

  const { github, written } = fakeGitHub(files)
  await updateDownloadBadge({ github, context, core })

  assert.deepEqual(written, [])
})
