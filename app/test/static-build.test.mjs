import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const buildRoot = new URL('../dist/', import.meta.url);

const routeChecks = [
  {
    file: 'index.html',
    text: [
      '<div id="app"></div>',
      '/assets/'
    ],
    absentText: [
      'Write mode',
      'Write command',
      'Set Project state',
      'Review pass',
      'Review reject',
      'Forge create',
      'Merge once',
      'ProjectV2 metadata refresh needs a routing decision',
      'Agent Review timed out before Human Review evidence',
      'Issue Forge draft is missing dependency semantics'
    ]
  }
];

test('static build contains the Vite Svelte operator app entrypoint only', (t) => {
  if (process.env.SHEA_SYMPHONY_APP_REQUIRE_BUILD !== '1') {
    t.skip('run npm run test:build for the strict static build smoke test');
    return;
  }

  if (!existsSync(buildRoot)) {
    assert.fail('dist/ is missing; run npm run build before the static build smoke test');
    return;
  }

  for (const route of routeChecks) {
    const path = join(buildRoot.pathname, route.file);
    assert.equal(existsSync(path), true, `${route.file} should exist`);
    const html = readFileSync(path, 'utf8');
    for (const expected of route.text) {
      assert.match(html, new RegExp(escapeRegExp(expected)), `${route.file} should include ${expected}`);
    }
    for (const blocked of route.absentText ?? []) {
      assert.doesNotMatch(html, new RegExp(escapeRegExp(blocked)), `${route.file} should not include ${blocked}`);
    }
  }
});

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
