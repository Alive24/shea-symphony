import { normalizeStateName } from './issueState.ts';

export type HumanHandoffState = 'Need to Clarify' | 'Need Human Input' | 'Human Review';
export type HandoffPromptTemplates = Record<HumanHandoffState, string>;

const humanHandoffStates = new Set<HumanHandoffState>([
  'Need to Clarify',
  'Need Human Input',
  'Human Review'
]);

export function buildHandoffPrompt(
  issue: Record<string, any>,
  templates: HandoffPromptTemplates
) {
  const state = normalizeStateName(issue?.state);
  if (!humanHandoffStates.has(state as HumanHandoffState)) {
    throw new Error(`No human handoff prompt is defined for state "${state}".`);
  }

  const template = templates[state as HumanHandoffState];
  if (!template?.trim()) {
    throw new Error(`The ${state} handoff prompt template is empty.`);
  }

  return renderHandoffTemplate(template, {
    issue: {
      identifier: String(issue?.id ?? 'unknown issue'),
      title: String(issue?.title ?? '').trim(),
      state,
      lane: String(issue?.lane ?? '').trim(),
      category: String(issue?.category ?? '').trim(),
      worker_status: String(issue?.workerStatus ?? '').trim(),
      worker_detail: String(issue?.workerDetail ?? '').trim(),
      recommended: String(issue?.recommended ?? '').trim(),
      evidence: String(issue?.evidence ?? '').trim(),
      url: String(issue?.url ?? '').trim()
    }
  });
}

export function renderHandoffTemplate(
  template: string,
  context: Record<string, any>
) {
  const conditionalPattern =
    /{%\s*if\s+([A-Za-z_][A-Za-z0-9_.]*)\s*%}([\s\S]*?){%\s*endif\s*%}/g;
  const variablePattern = /{{\s*([A-Za-z_][A-Za-z0-9_.]*)\s*}}/g;

  const withConditionals = template.replace(
    conditionalPattern,
    (_match, path: string, body: string) =>
      templateValue(context, path) ? body : ''
  );
  const rendered = withConditionals.replace(
    variablePattern,
    (_match, path: string) => String(templateValue(context, path) ?? '')
  );

  const unsupportedTag = rendered.match(/{[{%][\s\S]*?[}%]}/);
  if (unsupportedTag) {
    throw new Error(`Unsupported handoff prompt template tag: ${unsupportedTag[0]}`);
  }
  return rendered.trim();
}

function templateValue(context: Record<string, any>, path: string) {
  let value: any = context;
  for (const segment of path.split('.')) {
    if (
      value == null ||
      typeof value !== 'object' ||
      !Object.prototype.hasOwnProperty.call(value, segment)
    ) {
      throw new Error(`Unknown handoff prompt template variable: ${path}`);
    }
    value = value[segment];
  }
  return value;
}
