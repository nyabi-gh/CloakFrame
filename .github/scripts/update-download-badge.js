// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2026 Nyabi (nyabi-gh)

const badgeBranch = 'download-badge'
const badgePath = 'downloads.json'
// GitHub keeps only each file's running total, so this file's history on the badge branch is
// the one record of which files a rise in the badge came from.
const countsPath = 'downloads-by-file.json'

function isDistributionAsset(name)
{
  return /\.dmg$/i.test(name)
    || /Setup\.exe$/i.test(name)
    || /-windows-x64\.zip$/i.test(name)
    || /-macOS-arm64\.zip$/i.test(name)
    || /\.AppImage$/i.test(name)
    || /-full\.nupkg$/i.test(name)
}

function distributionAssets(releases)
{
  return releases
    .filter(release => !release.draft)
    .flatMap(release => release.assets
      .filter(asset => isDistributionAsset(asset.name))
      .map(asset => ({ release: release.tag_name, name: asset.name, downloads: asset.download_count })))
}

function distributionDownloadTotal(releases)
{
  return distributionAssets(releases).reduce((total, asset) => total + asset.downloads, 0)
}

function countsPayload(releases)
{
  const assets = distributionAssets(releases)
    .sort((a, b) => a.release.localeCompare(b.release) || a.name.localeCompare(b.name))
  return `${JSON.stringify(assets, null, 2)}\n`
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

async function currentFile(github, context, path)
{
  try
  {
    const response = await github.rest.repos.getContent({
      ...context.repo,
      path,
      ref: badgeBranch,
    })
    if (Array.isArray(response.data))
    {
      throw new Error(`${path} is not a file`)
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

async function listReleases(github, owner, repo)
{
  return await github.paginate(github.rest.repos.listReleases, { owner, repo, per_page: 100 })
}

async function writeIfChanged(github, context, path, content, message)
{
  const existing = await currentFile(github, context, path)
  if (existing.content === content)
  {
    return false
  }
  await github.rest.repos.createOrUpdateFileContents({
    ...context.repo,
    path,
    branch: badgeBranch,
    message,
    content: Buffer.from(content).toString('base64'),
    ...(existing.sha ? { sha: existing.sha } : {}),
  })
  return true
}

async function updateDownloadBadge({ github, context, core })
{
  const releases = await listReleases(github, context.repo.owner, context.repo.repo)
  const total = distributionDownloadTotal(releases)

  if (!await branchExists(github, context))
  {
    await github.rest.git.createRef({
      ...context.repo,
      ref: `refs/heads/${badgeBranch}`,
      sha: context.sha,
    })
  }

  if (await writeIfChanged(github, context, countsPath, countsPayload(releases),
    `Record downloads by file at ${total}`))
  {
    core.info(`Recorded downloads by file at ${total}.`)
  }
  if (await writeIfChanged(github, context, badgePath, badgePayload(total),
    `Update download badge to ${total}`))
  {
    core.info(`Updated download badge to ${total}.`)
  }
  else
  {
    core.info(`Download badge is already current at ${total}.`)
  }
}

module.exports = updateDownloadBadge
module.exports.badgePayload = badgePayload
module.exports.countsPayload = countsPayload
module.exports.distributionDownloadTotal = distributionDownloadTotal
module.exports.isDistributionAsset = isDistributionAsset
