import { expect, mock, test } from 'claude-code/testing'

import { parseSnTable, summarizeError, summarizeOutput } from './table'

const PANE_PROPS = {
  title: 'sn table',
  isFocused: false,
  bodyColumns: 60,
  placement: 'dock',
  scroll: { offset: 0, bodyRows: 40 },
  view: {},
} as const

test('parses explicit, implied and reference forms', async () => {
  expect(parseSnTable('sn table list incident -q active=true --setlimit 5')).toEqual({
    verb: 'list',
    table: 'incident',
    args: '-q active=true --setlimit 5',
    profile: null,
    isPiped: false,
  })
  expect(parseSnTable('sn -p devitil table incident')?.verb).toBe('list')
  expect(parseSnTable('sn -p devitil table incident')?.profile).toBe('devitil')
  expect(parseSnTable('sn table incident:INC0010001')).toMatchObject({
    verb: 'get',
    table: 'incident',
    args: 'INC0010001',
  })
  expect(parseSnTable('cd x && sn table get sys_user abc 2>&1 | head')).toMatchObject({
    verb: 'get',
    table: 'sys_user',
    isPiped: true,
  })
  expect(parseSnTable('sn change list')).toBeNull()
  expect(parseSnTable('grep "sn table" README.md')).toBeNull()
})

test('summarizes arrays, records, JSONL and errors', async () => {
  expect(summarizeOutput('[{"sys_id":"a"},{"sys_id":"b"}]', '', false)).toMatchObject({
    summary: '2 records',
    records: 2,
  })
  expect(summarizeOutput('{"sys_id":"a","number":"INC0010001"}', '', false).summary).toBe(
    '1 record · INC0010001',
  )
  expect(
    summarizeOutput('{"number":"INC0000016","priority":"1 - Critical"}', '', false, 'get'),
  ).toMatchObject({ summary: '1 record · INC0000016', records: 1 })
  expect(summarizeOutput('{"sys_id":"a"}\n{"sys_id":"b"}\n{"sys_id":"c"}\n', '', false).records).toBe(3)
  expect(summarizeOutput('3', '', true)).toMatchObject({ summary: 'piped · 3', records: null })
  expect(
    summarizeError('Exit code 2\n{"error":{"message":"Invalid table foo","status_code":400}}').summary,
  ).toBe('exit 2 API · Invalid table foo (400)')
})

test('a Bash sn table call lands in the totals and the pane', async ($, on) => {
  mock.clock(on, { now: 1000 })
  const failure = 'Exit code 2\n{"error":{"message":"Invalid table foo","status_code":400}}'
  on('tool.call', { tool: 'Bash' }, ($, e) =>
    e.command.includes(' foo')
      ? { isError: true, result: failure, text: failure }
      : { result: { stdout: '[{"sys_id":"a"},{"sys_id":"b"}]', stderr: '', interrupted: false } },
  )

  await $.tool.call({ tool: 'Bash', command: 'sn table list incident -q active=true' })
  await $.tool.call({ tool: 'Bash', command: 'git status' })
  await $.tool.call({ tool: 'Bash', command: 'sn table list foo' })

  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({
      plugin: 'sn',
      surface,
      component: 'Pane',
      requestId: 'sn-table',
      props: PANE_PROPS,
    })
    expect(await ui.find({ type: 'Text', text: /2 calls · 1 ok · 1 failed · 2 records/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /✓ list incident/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /exit 2 API · Invalid table foo \(400\)/ })).toBeDefined()
    await ui.unmount()
  }

  const ui = await $.ui.mount({
    plugin: 'sn',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'sn-table',
    props: PANE_PROPS,
  })
  await ui.press({ key: 'clear' })
  expect(await ui.find({ type: 'Text', text: /No sn table commands yet/ })).toBeDefined()
  await ui.unmount()
})
