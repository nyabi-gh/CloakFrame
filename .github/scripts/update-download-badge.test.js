// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

const assert = require('node:assert/strict')
const test = require('node:test')

const {
  badgePayload,
  distributionDownloadTotal,
  isDistributionAsset,
} = require('./update-download-badge.js')

test('includes user downloads and full updater packages', () =>
{
  for (const name of [
    'CloakFrame-macOS-arm64.dmg',
    'CloakFrame-Windows-x64-Setup.exe',
    'CloakFrame-1.10.2-windows-x64.zip',
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
