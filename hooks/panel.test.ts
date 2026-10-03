import { expect, mock, test } from 'claude-code/testing'

import { openArgv, parseSnTable, summarizeError, summarizeOutput } from './table'
import type { Outcome } from './table'

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
    bin: 'sn',
    verb: 'list',
    table: 'incident',
    record: null,
    query: 'active=true',
    args: '-q active=true --setlimit 5',
    profile: null,
    isPiped: false,
  })
  expect(parseSnTable('sn -p devitil table incident')?.verb).toBe('list')
  expect(parseSnTable('sn -p devitil table incident')?.profile).toBe('devitil')
  expect(parseSnTable('sn table incident:INC0010001')).toMatchObject({
    verb: 'get',
    table: 'incident',
    record: 'INC0010001',
    args: 'INC0010001',
  })
  expect(parseSnTable('sn table get incident 0123abcd -f number')?.record).toBe('0123abcd')
  expect(parseSnTable('sn table list incident --query=priority=1^active=true')?.query).toBe(
    'priority=1^active=true',
  )
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

test('Open shows the list view or the record form, never a failure or a delete', async () => {
  const ok: Outcome = { status: 'ok', summary: '', records: 1, exitCode: 0, sysId: null }
  const parse = (command: string) => {
    const call = parseSnTable(command)
    if (call === null) throw new Error(`no sn table in ${command}`)
    return call
  }

  expect(openArgv(parse('sn table list incident -q active=true'), ok)).toEqual([
    'sn',
    'open',
    'incident',
    '-q',
    'active=true',
  ])
  expect(openArgv(parse('sn table problem'), ok)).toEqual(['sn', 'open', 'problem'])
  expect(openArgv(parse('sn -p devitil table incident:INC0000016'), ok)).toEqual([
    'sn',
    'open',
    'incident:INC0000016',
    '-p',
    'devitil',
  ])
  expect(openArgv(parse('sn table create incident -f short_description=x'), { ...ok, sysId: 'abc' })).toEqual([
    'sn',
    'open',
    'incident:abc',
  ])
  expect(openArgv(parse('sn table delete incident abc --yes'), ok)).toBeNull()
  expect(openArgv(parse('sn table list foo'), { ...ok, status: 'failed' })).toBeNull()
})

test('a Bash sn table call lands in the totals and the pane', async ($, on) => {
  mock.clock(on, { now: 1000 })
  const ran: (readonly string[])[] = []
  const toasts: string[] = []
  on('process.run', ($, e) => {
    ran.push(e.argv)
    return {
      value: { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
    }
  })
  on('ui.toast', ($, e) => {
    toasts.push(e.text)
    return { value: undefined }
  })
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
  const opens = await ui.findAll({ type: 'Button', text: 'Open' })
  expect(opens.length).toBe(1)

  await ui.press({ key: opens[0]?.key ?? '' })
  expect(ran).toEqual([['sn', 'open', 'incident', '-q', 'active=true']])
  expect(toasts).toEqual(['Opened the incident list in the browser'])

  await ui.press({ key: 'clear' })
  expect(await ui.find({ type: 'Text', text: /No sn table commands yet/ })).toBeDefined()
  await ui.unmount()
})
