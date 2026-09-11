import test from 'node:test';
import assert from 'node:assert/strict';
import { reviewLifecycle } from '../src/lib/reviewActions.ts';

test('a tracker handoff does not prove Review dispatch', () => {
  const result = reviewLifecycle({ issues: [{ identifier: '#5', state: 'Agent Review' }] }, '#5');
  assert.equal(result.label, 'Not dispatched');
  assert.equal(result.canStart, true);
  assert.equal(result.canRecover, false);
  assert.equal(reviewLifecycle(null, '#5').canStart, false);
});

test('a terminal backend job never implies published evidence', () => {
  const result = reviewLifecycle({ recent_jobs: [{ issue_identifier: '#5', job_state: 'Completed', review_outcome: 'Pass' }] }, '#5');
  assert.equal(result.label, 'Publication unverified');
  assert.equal(result.canStart, false);
});

test('captured terminal publication can recover without another dispatch', () => {
  for (const state of ['ResultReady', 'EvidencePublished']) {
    const result = reviewLifecycle({ pending_publications: [{ issue_ref: '#5', state, terminal_result_captured: true, run_id: 'same-run' }] }, '#5');
    assert.equal(result.label, 'Publication pending');
    assert.equal(result.canStart, false);
    assert.equal(result.canRecover, true);
    assert.equal(result.runId, 'same-run');
  }
});

test('starting, malformed and superseded receipts never allow automatic retry', () => {
  for (const receipt of [{state:'Starting'}, {state:'Running'}, {state:'Superseded'}, {diagnostic:'corrupt receipt'}]) {
    const result = reviewLifecycle({ pending_publications: [{ issue_ref:'#5', ...receipt }] }, '#5');
    assert.equal(result.canStart, false);
    assert.equal(result.canRecover, false);
  }
});

test('completion requires a final receipt for the selected Issue', () => {
  const status = { publications: [{issue_ref:'#7',state:'Complete',terminal_result_captured:true},{issue_ref:'#5',state:'Complete',terminal_result_captured:true,run_id:'published'}] };
  assert.equal(reviewLifecycle(status, '#5').label, 'Publication complete');
  assert.equal(reviewLifecycle(status, '#8').label, 'Not dispatched');
  const pending = {...status,pending_publications:[{issue_ref:'#5',state:'ResultReady',terminal_result_captured:true}]};
  assert.equal(reviewLifecycle(pending, '#5').label, 'Publication pending');
});

test('an incomplete Complete receipt is not accepted as publication evidence', () => {
  const result = reviewLifecycle({publications:[{issue_ref:'#5',state:'Complete',terminal_result_captured:false}]}, '#5');
  assert.equal(result.label, 'Needs diagnosis');
  assert.equal(result.canStart, false);
});
