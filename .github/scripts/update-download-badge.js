// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

const badgeBranch = 'download-badge'
const badgePath = 'downloads.json'

function isDistributionAsset(name)
{
  return /\.dmg$/i.test(name)
    || /Setup\.exe$/i.test(name)
    || /-windows-x64\.zip$/i.test(name)
    || /\.AppImage$/i.test(name)
    || /-full\.nupkg$/i.test(name)
}

function distributionDownloadTotal(releases)
{
  return releases
    .filter(release => !release.draft)
    .flatMap(release => release.assets)
    .filter(asset => isDistributionAsset(asset.name))
    .reduce((total, asset) => total + asset.download_count, 0)
}

function badgePayload(total)
{
  return `${JSON.stringify({
    schemaVersion: 1,
    label: 'downloads',
    message: String(total),
    color: '6366f1',
  }, null, 2)}\n`
}

async function branchExists(github, context)
{
  try
  {
    await github.rest.git.getRef({
      ...context.repo,
      ref: `heads/${badgeBranch}`,
    })
    return true
  }
  catch (error)
  {
    if (error.status !== 404)
    {
      throw error
    }
    return false
  }
}

async function currentBadge(github, context)
{
  try
  {
    const response = await github.rest.repos.getContent({
      ...context.repo,
      path: badgePath,
      ref: badgeBranch,
    })
    if (Array.isArray(response.data))
    {
      throw new Error(`${badgePath} is not a file`)
    }
    return {
      content: Buffer.from(response.data.content, 'base64').toString('utf8'),
      sha: response.data.sha,
    }
  }
  catch (error)
  {
    if (error.status !== 404)
    {
      throw error
    }
    return { content: '', sha: undefined }
  }
}

async function updateDownloadBadge({ github, context, core })
{
  const releases = await github.paginate(github.rest.repos.listReleases, {
    ...context.repo,
    per_page: 100,
  })
  const total = distributionDownloadTotal(releases)
  const nextContent = badgePayload(total)

  if (!await branchExists(github, context))
  {
    await github.rest.git.createRef({
      ...context.repo,
      ref: `refs/heads/${badgeBranch}`,
      sha: context.sha,
    })
  }

  const existing = await currentBadge(github, context)
  if (existing.content === nextContent)
  {
    core.info(`Download badge is already current at ${total}.`)
    return
  }

  await github.rest.repos.createOrUpdateFileContents({
    ...context.repo,
    path: badgePath,
    branch: badgeBranch,
    message: `Update download badge to ${total}`,
    content: Buffer.from(nextContent).toString('base64'),
    ...(existing.sha ? { sha: existing.sha } : {}),
  })
  core.info(`Updated download badge to ${total}.`)
}

module.exports = updateDownloadBadge
module.exports.badgePayload = badgePayload
module.exports.distributionDownloadTotal = distributionDownloadTotal
module.exports.isDistributionAsset = isDistributionAsset
