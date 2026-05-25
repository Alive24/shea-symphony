import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const buildRoot = new URL('../build/', import.meta.url);

const routeChecks = [
  {
    file: 'index.html',
    text: [
      'No human to-do issues visible',
      'Human operator issue queue',
      'Toggle Live and Fixture data',
      'Toggle Day and Night theme',
      'Main',
      'Review',
      'Merge'
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
  },
  {
    file: 'lanes.html',
    text: ['Issue Index', 'Cross-Lane Evidence', 'State Pressure', 'Project / worker match']
  },
  {
    file: 'events.html',
    text: ['Evidence Map', 'Signals by Lane', 'Audit trail']
  },
  {
    file: 'runbook.html',
    text: ['Skill Routing', 'Chat-Led Operations Map', 'Responsibility Matrix', 'State Routing', 'Human gates']
  },
  {
    file: 'doctor.html',
    text: ['Doctor', 'Diagnostic Readback', 'Readback Console', 'No writes', 'Read/dry-run command']
  },
  {
    file: 'settings.html',
    text: ['Settings', 'Read Surface Matrix', 'Tracker authority']
  }
];

test('static build contains the operator visualization routes', (t) => {
  if (process.env.SHEA_WEB_REQUIRE_BUILD !== '1') {
    t.skip('run npm run test:build for the strict static build smoke test');
    return;
  }

  if (!existsSync(buildRoot)) {
    assert.fail('build/ is missing; run npm run build before the static build smoke test');
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
