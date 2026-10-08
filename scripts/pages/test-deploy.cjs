const { test } = require('node:test');
const assert = require('node:assert/strict');
const { buildVersion, deploy } = require('./deploy.cjs');

test('main, tag and retry deployments on the same commit have different versions', () => {
  assert.notEqual(buildVersion('sha', 1, 1, 10), buildVersion('sha', 2, 1, 20));
  assert.notEqual(buildVersion('sha', 1, 1, 10), buildVersion('sha', 1, 2, 11));
});

function fixture(statuses) {
  const calls = [], outputs = {};
  let time = 0;
  return {
    calls, outputs,
    options: {
      context: { repo: { owner: 'owner', repo: 'repo' }, sha: 'sha', runId: 1 },
      artifactId: '10', attempt: 1, timeout: 10_000,
      now: () => time, sleep: async ms => { time += ms; },
      core: { getIDToken: async () => 'oidc', setOutput: (k, v) => { outputs[k] = v; }, info: () => {} },
      github: { request: async (route, payload) => {
        calls.push([route, payload]);
        return { data: route.startsWith('GET') ? { status: statuses.shift() || 'queued' }
          : { id: 'deployment', page_url: 'https://example.invalid/' } };
      } },
    },
  };
}

test('waits for success before exposing URL', async () => {
  const f = fixture(['queued', 'succeed']);
  await deploy(f.options);
  assert.equal(f.outputs.page_url, 'https://example.invalid/');
  assert.equal(f.calls[0][1].artifact_id, 10);
  assert.equal(f.calls[0][1].pages_build_version, buildVersion('sha', 1, 1, 10));
});

test('deployment failure stays a failure', async () => {
  const f = fixture(['deployment_content_failed']);
  await assert.rejects(deploy(f.options), /deployment_content_failed/);
  assert.deepEqual(f.outputs, {});
});

test('timeout cancels the deployment and fails', async () => {
  const f = fixture([]);
  await assert.rejects(deploy(f.options), /timed out/);
  assert.match(f.calls.at(-1)[0], /\/cancel$/);
  assert.deepEqual(f.outputs, {});
});
