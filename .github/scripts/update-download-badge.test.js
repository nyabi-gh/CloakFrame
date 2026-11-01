// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2026 Nyabi (nyabi-gh)

const assert = require('node:assert/strict')
const test = require('node:test')

const updateDownloadBadge = require('./update-download-badge.js')
const {
  badgePayload,
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

test('writes the badge from this repository\'s releases', async () =>
{
  const listed = []
  const written = []
  const github = {
    paginate: async (_method, { owner, repo }) =>
    {
      listed.push(`${owner}/${repo}`)
      return [
        {
          draft: false,
          assets: [{ name: 'CloakFrame-2.0.0-Windows-x64-Setup.exe', download_count: 4 }],
        },
      ]
    },
    rest: {
      repos: {
        listReleases: {},
        getContent: async () =>
        {
          throw Object.assign(new Error('Not Found'), { status: 404 })
        },
        createOrUpdateFileContents: async request => written.push(request),
      },
      git: {
        getRef: async () => ({}),
      },
    },
  }
  const context = { repo: { owner: 'nyabi-gh', repo: 'CloakFrame' }, sha: 'abc' }
  const core = { info: () => {} }

  await updateDownloadBadge({ github, context, core })

  assert.deepEqual(listed, ['nyabi-gh/CloakFrame'])
  assert.equal(written.length, 1)
  assert.equal(JSON.parse(Buffer.from(written[0].content, 'base64').toString('utf8')).message, '4')
})
