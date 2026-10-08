// Use GitHub's Pages API with a distinct build version for every artifact/attempt.
const { createHash } = require('node:crypto');

function buildVersion(sha, runId, attempt, artifactId) {
  return createHash('sha1').update(`${sha}:${runId}:${attempt}:${artifactId}`).digest('hex');
}

async function deploy({ github, core, context, artifactId, attempt,
  timeout = 600_000, sleep = ms => new Promise(resolve => setTimeout(resolve, ms)),
  now = Date.now }) {
  const repository = context.repo;
  const { data: deployment } = await github.request('POST /repos/{owner}/{repo}/pages/deployments', {
    ...repository,
    artifact_id: Number(artifactId),
    pages_build_version: buildVersion(context.sha, context.runId, attempt, artifactId),
    oidc_token: await core.getIDToken(),
  });
  const deadline = now() + timeout;
  while (now() < deadline) {
    const { data } = await github.request('GET /repos/{owner}/{repo}/pages/deployments/{deployment_id}', {
      ...repository, deployment_id: deployment.id,
    });
    if (data.status === 'succeed') {
      core.setOutput('page_url', deployment.page_url);
      core.info('Pages deployment completed; public content verification follows.');
      return;
    }
    if (['deployment_failed', 'deployment_content_failed', 'deployment_cancelled',
      'deployment_lost', 'deployment_system_failed', 'failed', 'cancelled'].includes(data.status)) {
      throw new Error(`Pages deployment failed: ${data.status}`);
    }
    core.info(`Pages deployment: ${data.status}`);
    await sleep(5000);
  }
  await github.request('POST /repos/{owner}/{repo}/pages/deployments/{deployment_id}/cancel', {
    ...repository, deployment_id: deployment.id,
  });
  throw new Error('Pages deployment timed out and was cancelled');
}
module.exports = { buildVersion, deploy };
