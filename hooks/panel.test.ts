import { expect, mock, test } from 'claude-code/testing'

import { kindOf } from './kinds'
import { openArgv, summarizeError, summarizeOutput } from './outcome'
import type { Outcome } from './outcome'
import { parseSn, teeCommand } from './parse'

const PANE_PROPS = {
  title: 'sn',
  isFocused: false,
  bodyColumns: 60,
  placement: 'dock',
  scroll: { offset: 0, bodyRows: 40 },
  view: {},
} as const

const parse = (command: string) => {
  const call = parseSn(command)
  if (call === null) throw new Error(`no sn in ${command}`)
  return call
}

test('parses explicit, implied and reference forms of sn table', async () => {
  expect(parseSn('sn table list incident -q active=true --setlimit 5')).toEqual({
    bin: 'sn',
    path: ['table', 'list'],
    command: 'table list',
    table: 'incident',
    record: null,
    query: 'active=true',
    operands: { table: 'incident' },
    args: '-q active=true --setlimit 5',
    profile: null,
    isPiped: false,
    pipeAt: null,
    stdinPipeAt: null,
    heredoc: null,
    isDynamic: false,
  })
  expect(parse('sn -p devitil table incident')).toMatchObject({ command: 'table list', profile: 'devitil' })
  expect(parse('sn table incident:INC0010001')).toMatchObject({
    command: 'table get',
    table: 'incident',
    record: 'INC0010001',
    args: 'INC0010001',
  })
  expect(parse('sn table get incident 0123abcd -f number').record).toBe('0123abcd')
  expect(parse('sn table list incident --query=priority=1^active=true').query).toBe('priority=1^active=true')
  expect(parse('cd x && sn table get sys_user abc 2>&1 | head')).toMatchObject({
    command: 'table get',
    table: 'sys_user',
    isPiped: true,
  })
  expect(parseSn('grep "sn table" README.md')).toBeNull()
  expect(parseSn('git status')).toBeNull()
})

test('parses every command group from the catalog', async () => {
  expect(parse('sn change list -q active=true')).toMatchObject({ command: 'change list', query: 'active=true' })
  expect(parse('sn change task get abc def')).toMatchObject({
    command: 'change task get',
    operands: { change_sys_id: 'abc', task_sys_id: 'def' },
    record: 'def',
  })
  expect(parse('sn cmdb cmdb_ci_server')).toMatchObject({ command: 'cmdb list', table: 'cmdb_ci_server' })
  expect(parse('sn cmdb get cmdb_ci_server abc')).toMatchObject({ table: 'cmdb_ci_server', record: 'abc' })
  expect(parse('sn get incident:INC0010001')).toMatchObject({ command: 'get', table: 'incident', record: 'INC0010001' })
  expect(parse(`sn script run 'gs.info(1)' --scope x_app --yes`)).toMatchObject({
    command: 'script run',
    operands: { script: 'gs.info(1)' },
    args: '--scope x_app --yes',
  })
  expect(parse(`sn script run @- --yes <<'EOF'\ngs.info(1)\ngs.info(2)\nEOF`).heredoc).toBe('gs.info(1)\ngs.info(2)')
  expect(parse(`printf 'gs.info(1)' | sn script run @- --yes`).stdinPipeAt).toBe(20)
  expect(parse('echo x | sn script run @- --yes').command).toBe('script run')
  expect(parse('sn --timeout 60 watch incident -q active=true')).toMatchObject({ command: 'watch', table: 'incident' })
  expect(parse('sn profile list').command).toBe('profile list')
  expect(parse('sn ping').command).toBe('ping')
  expect(parse('sn --version').command).toBe('sn')
  expect(parse('/usr/local/bin/sn flow tail abc').bin).toBe('/usr/local/bin/sn')
  // Redirections are the shell's: never an operand, never shown.
  expect(parse('sn table incident 2>&1')).toMatchObject({ command: 'table list', args: '' })
  expect(parse('sn profile list 2>/dev/null > out.json')).toMatchObject({ command: 'profile list', args: '' })
  expect(parse('sn table list incident -q "priority>1" > out.json').query).toBe('priority>1')
})

test('a tee goes between sn and its pipe, nowhere else', async () => {
  const command = `cd x && sn table list incident -q 'a|b' 2>&1 | jq length`
  const call = parse(command)
  expect(call.pipeAt).not.toBeNull()
  expect(teeCommand(command, call.pipeAt ?? 0, "/tmp/it's.out")).toBe(
    `cd x && sn table list incident -q 'a|b' 2>&1 | tee '/tmp/it'\\''s.out' | jq length`,
  )
  expect(parse('sn table list incident').pipeAt).toBeNull()
  expect(parse('sn table list incident || true').pipeAt).toBeNull()
  expect(parse('sn table list incident |& less').pipeAt).toBeNull()
})

test('reads -q wherever and however clap accepts it', async () => {
  for (const command of [
    'sn table list incident -q active=true',
    'sn table list incident -qactive=true',
    'sn table list incident -q=active=true',
    'sn table list incident -dq active=true',
    'sn table list incident --sysparm-query active=true',
    'sn table list -q active=true incident',
    'sn table -q active=true incident',
    'sn table list --setlimit 5 -f number,state incident -q active=true',
  ]) {
    expect(parseSn(command)).toMatchObject({ command: 'table list', table: 'incident', query: 'active=true' })
  }
  expect(parse('sn table incident \\\n  -q "active=true^priority=1" \\\n  -f number')).toMatchObject({
    command: 'table list',
    query: 'active=true^priority=1',
    args: '-q "active=true^priority=1" -f number',
  })
  expect(parse('sn table list -q active=true incident --all').args).toBe('-q active=true --all')
  expect(parse('sn table get incident -f number 0123abcd')).toMatchObject({ record: '0123abcd' })
  expect(parse(`sn table list incident -q 'a=$1' -f "x\\y"`)).toMatchObject({ query: 'a=$1', isDynamic: false })
})

test('classifies each command into the kind that sets its look', async () => {
  const kinds: Record<string, string> = {
    'sn table list incident': 'read',
    'sn table incident:INC1': 'read',
    'sn table create incident -F a=b': 'write',
    'sn change task update abc def -F a=b': 'write',
    'sn table delete incident abc --yes': 'delete',
    'sn change conflict remove abc': 'delete',
    'sn cicd app rollback --sys-id abc': 'delete',
    'sn script run x --yes': 'run',
    'sn cicd atf run --suite-id abc': 'run',
    'sn watch incident': 'stream',
    'sn flow tail abc': 'stream',
    'sn profile remove dev': 'session',
    'sn context scope x_app': 'session',
    'sn set updateset fixes': 'session',
    'sn get scope': 'read',
    'sn get INC0010001': 'read',
    'sn raw GET /api/now/table/incident': 'read',
    'sn raw DELETE /api/now/table/incident/abc': 'delete',
    'sn raw POST /api/now/table/incident': 'write',
    'sn graphql "mutation { x }"': 'write',
    'sn graphql "{ x }"': 'read',
    'sn ping': 'read',
  }
  for (const [command, kind] of Object.entries(kinds)) {
    expect([command, kindOf(parse(command))]).toEqual([command, kind])
  }
})

test('summarizes by kind: reads count, writes say what they did', async () => {
  const list = parse('sn table list incident')
  expect(summarizeOutput('[{"number":"INC1"},{"number":"INC2"}]', '', list, 'read')).toMatchObject({
    summary: '2 records · INC1, INC2',
    records: 2,
  })
  expect(summarizeOutput('[{"sys_id":"a"},{"sys_id":"b"},{"sys_id":"c"},{"sys_id":"d"}]', '', list, 'read').summary).toBe(
    '4 records · a, b, c, …',
  )
  // Text reads as its first line, never as a count of lines.
  const help = parse('sn script --help 2>&1 | head -30')
  expect(summarizeOutput('Run server-side JavaScript\n\nUsage: sn script <COMMAND>\n', '', help, 'read').summary).toBe(
    'piped · Run server-side JavaScript',
  )
  // Pretty JSON, and a nested count.
  const aggregate = parse('sn aggregate incident --count')
  expect(summarizeOutput('{\n  "stats": {\n    "count": "44"\n  }\n}\n', '', aggregate, 'read').summary).toBe(
    'count=44',
  )
  expect(summarizeOutput('{"sys_id":"a"}\n{"sys_id":"b"}\n{"sys_id":"c"}\n', '', list, 'read').records).toBe(3)
  expect(summarizeOutput('3', '', parse('sn table list incident | wc -l'), 'read')).toMatchObject({
    summary: 'piped · 3',
    records: null,
  })
  const get = parse('sn table get incident abc -f number,priority')
  expect(summarizeOutput('{"number":"INC0000016","priority":"1 - Critical"}', '', get, 'read')).toMatchObject({
    summary: '1 record · INC0000016',
    records: 1,
  })
  const create = parse('sn table create incident -F short_description=x')
  expect(summarizeOutput('{"sys_id":"abc","number":"INC0010023"}', '', create, 'write')).toMatchObject({
    summary: 'created · INC0010023',
    records: null,
    sysId: 'abc',
  })
  const remove = parse('sn table delete incident abc --yes')
  expect(summarizeOutput('', '', remove, 'delete').summary).toBe('deleted · incident:abc')
  const watch = parse('sn watch incident')
  expect(summarizeOutput('{"op":"insert"}\n{"op":"update"}\n', '', watch, 'stream').summary).toMatch(/^2 events/)
  expect(
    summarizeError('Exit code 2\n{"error":{"message":"Invalid table foo","status_code":400}}').summary,
  ).toBe('exit 2 API · Invalid table foo (400)')
})

test('a script run shows what it logged, and where it failed', async () => {
  const ok =
    '{"elapsed_ms":124,"error":null,"history_id":"h1","messages":["src: noted"],"ok":true,' +
    '"output":["hello","world"],"rollback_context":null,"scope":"x_app"}'
  const call = parse(`sn script run 'gs.info("hello")' --yes`)
  expect(summarizeOutput(ok, '', call, 'run')).toEqual({
    status: 'ok',
    summary: 'completed · x_app · hello',
    lines: ['hello', 'world', '· src: noted'],
    records: null,
    exitCode: 0,
    sysId: 'h1',
  })

  const failed =
    'Exit code 2\n' +
    '{"elapsed_ms":86,"error":{"detail":"…","line":1,"message":"Cannot convert null to an object.","type":"execution"},' +
    '"history_id":"h2","messages":[],"ok":false,"output":["before"],"rollback_context":null,"scope":"global"}\n' +
    '{"error":{"detail":"line 1","message":"script execution error: Cannot convert null to an object.","status_code":200}}'
  const outcome = summarizeError(failed, 'script run')
  expect(outcome).toMatchObject({
    status: 'failed',
    summary: 'exit 2 API · execution error line 1 · Cannot convert null to an object.',
    lines: ['before'],
    sysId: 'h2',
  })
  // The execution history row holds the error, so a failed run still opens.
  expect(openArgv(call, outcome, 'run')).toEqual(['sn', 'open', 'sys_script_execution_history:h2'])
})

test('Open shows the list view or the record form, never a failure or a delete', async () => {
  const ok: Outcome = { status: 'ok', summary: '', lines: [], records: 1, exitCode: 0, sysId: null }
  const open = (command: string, outcome: Outcome = ok) => {
    const call = parse(command)
    return openArgv(call, outcome, kindOf(call))
  }

  expect(open('sn table list incident -q active=true')).toEqual(['sn', 'open', 'incident', '-q', 'active=true'])
  expect(open('sn table problem')).toEqual(['sn', 'open', 'problem'])
  expect(open('sn -p devitil table incident:INC0000016')).toEqual([
    'sn',
    'open',
    'incident:INC0000016',
    '-p',
    'devitil',
  ])
  expect(open('sn table create incident -F short_description=x', { ...ok, sysId: 'abc' })).toEqual([
    'sn',
    'open',
    'incident:abc',
  ])
  expect(open('sn cmdb get cmdb_ci_server abc')).toEqual(['sn', 'open', 'cmdb_ci_server:abc'])
  expect(open('sn change get abc')).toEqual(['sn', 'open', 'change_request:abc'])
  expect(open('sn watch incident -q active=true')).toEqual(['sn', 'open', 'incident', '-q', 'active=true'])
  expect(open('sn get incident:INC0000016')).toEqual(['sn', 'open', 'incident:INC0000016'])
  expect(open('sn table delete incident abc --yes')).toBeNull()
  expect(open('sn profile list')).toBeNull()
  expect(open('sn ping')).toBeNull()
  // A query, table or profile the shell filled in is not known here.
  for (const command of [
    'sn table list incident -q "$Q"',
    'sn table list incident -q "sys_created_on>$(date +%F)"',
    'sn table list incident -q $(cat query.txt)',
    'sn table list incident -q `cat query.txt`',
    'sn table list "$TABLE"',
    'sn -p "$P" table list incident',
  ]) {
    expect(open(command)).toBeNull()
  }
  expect(open('sn table get "incident:$N"', { ...ok, sysId: 'abc' })).toEqual(['sn', 'open', 'incident:abc'])
  expect(open('sn table update incident "$ID" -F state=2')).toBeNull()
  expect(open('sn table list foo', { ...ok, status: 'failed' })).toBeNull()
})

test('any sn call lands in the pane, drawn by its kind', async ($, on) => {
  mock.clock(on, { now: 1000 })
  const ran: (readonly string[])[] = []
  const toasts: string[] = []
  const opened: string[] = []
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
  on('ui.open', ($, e) => {
    opened.push(e.id)
    return { value: { isPlaced: true } }
  })
  const failure = 'Exit code 2\n{"error":{"message":"Invalid table foo","status_code":400}}'
  const script =
    '{"elapsed_ms":12,"error":null,"history_id":"h1","messages":[],"ok":true,"output":["hello from sn"],' +
    '"rollback_context":null,"scope":"global"}'
  on('tool.call', { tool: 'Bash' }, ($, e) => {
    if (e.command.includes(' foo')) return { isError: true, result: failure, text: failure }
    if (e.command.includes('script run')) return { result: { stdout: script, stderr: '', interrupted: false } }
    if (e.command.includes('delete')) return { result: { stdout: '', stderr: '', interrupted: false } }
    return { result: { stdout: '[{"sys_id":"a"},{"sys_id":"b"}]', stderr: '', interrupted: false } }
  })

  await $.tool.call({ tool: 'Bash', command: 'sn table list incident -q active=true' })
  await $.tool.call({ tool: 'Bash', command: 'git status' })
  await $.tool.call({ tool: 'Bash', command: 'sn table list foo' })
  await $.tool.call({ tool: 'Bash', command: `sn script run 'gs.info("hello from sn")' --yes` })
  await $.tool.call({ tool: 'Bash', command: 'sn table delete incident abc --yes' })
  expect(opened).toEqual(['sn', 'sn', 'sn', 'sn'])

  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ plugin: 'sn', surface, component: 'Pane', requestId: 'sn', props: PANE_PROPS })
    expect(await ui.find({ type: 'Text', text: /4 calls · 3 ok · 1 failed · 2 records/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /◇ table list/ })).toBeDefined()
    expect(await ui.find({ type: 'Button', text: /▸ script run/ })).toBeDefined()
    expect(await ui.find({ type: 'Button', text: /hello from sn/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /✕ table delete/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /exit 2 API · Invalid table foo \(400\)/ })).toBeDefined()
    expect(await ui.find({ type: 'Button', text: /▶ run 1/ })).toBeDefined()
    await ui.unmount()
  }

  const ui = await $.ui.mount({
    plugin: 'sn',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'sn',
    props: PANE_PROPS,
  })
  const opens = await ui.findAll({ type: 'Button', text: 'Open' })
  expect(opens.length).toBe(1)

  await ui.press({ key: opens[0]?.key ?? '' })
  expect(ran).toEqual([['sn', 'open', 'incident', '-q', 'active=true']])
  expect(toasts).toEqual(['Opened the incident list in the browser'])

  // The kind chips filter the list.
  await ui.press({ key: 'kind-run' })
  expect(await ui.find({ type: 'Text', text: /◇ table list/ })).toBeUndefined()
  expect(await ui.find({ type: 'Button', text: /▸ script run/ })).toBeDefined()
  await ui.press({ key: 'kind-delete' })
  expect(await ui.find({ type: 'Button', text: /▸ script run/ })).toBeUndefined()
  expect(await ui.find({ type: 'Text', text: /✕ table delete/ })).toBeDefined()

  await ui.press({ key: 'clear' })
  expect(await ui.find({ type: 'Text', text: /No sn commands yet/ })).toBeDefined()
  await ui.unmount()
})

test('loading the sn skill opens the pane and lands an entry', async ($, on) => {
  mock.clock(on, { now: 1000 })
  const opened: string[] = []
  on('ui.open', ($, e) => {
    opened.push(e.id)
    return { value: { isPlaced: true } }
  })
  on('skill.prompt', ($, e) => ({ text: e.text }))

  await $.skill.prompt({ skill: 'commit', text: 'x' })
  expect(opened).toEqual([])
  await $.skill.prompt({ skill: 'sn:sn', text: 'x' })
  expect(opened).toEqual(['sn'])

  const ui = await $.ui.mount({
    plugin: 'sn',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'sn',
    props: PANE_PROPS,
  })
  expect(await ui.find({ type: 'Text', text: /★ skill sn:sn/ })).toBeDefined()
  expect(await ui.find({ type: 'Button', text: /★ skill 1/ })).toBeDefined()
  await ui.unmount()
})

test('a failed Open is logged with the argv, stderr and the sn that answered', async ($, on) => {
  mock.clock(on, { now: 1000 })
  mock.env(on, { HOME: '/home/me', PATH: '/usr/bin' })
  const files = new Map<string, string>([['/home/me/.claude/sn-panel.log', '{"event":"earlier"}\n']])
  const toasts: string[] = []
  const stderr = `{"error":{"message":"error: unexpected argument '-q' found"}}`
  on('fs.exists', ($, e) => ({ value: files.has(e.path) }))
  on('fs.read', ($, e) => ({ value: files.get(e.path) ?? '' }))
  on('fs.write', ($, e) => {
    files.set(e.path, e.text)
    return { value: undefined }
  })
  on('process.run', ($, e) => ({
    value: {
      exitCode: e.argv[1] === '--version' ? 0 : 1,
      stdout: e.argv[1] === '--version' ? 'sn 0.13.1\n' : '',
      stderr: e.argv[1] === '--version' ? '' : stderr,
      isStdoutTruncated: false,
      isStderrTruncated: false,
    },
  }))
  on('ui.toast', ($, e) => {
    toasts.push(e.text)
    return { value: undefined }
  })
  on('tool.call', { tool: 'Bash' }, () => ({
    result: { stdout: '[{"sys_id":"a"}]', stderr: '', interrupted: false },
  }))

  await $.tool.call({ tool: 'Bash', command: 'sn table list incident -q active=true' })
  const ui = await $.ui.mount({
    plugin: 'sn',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'sn',
    props: PANE_PROPS,
  })
  const open = await ui.find({ type: 'Button', text: 'Open' })
  await ui.press({ key: open?.key ?? '' })
  await ui.unmount()

  expect(toasts).toEqual([
    "sn open: exit 1 usage · error: unexpected argument '-q' found · logged to /home/me/.claude/sn-panel.log",
  ])
  const lines = (files.get('/home/me/.claude/sn-panel.log') ?? '').trim().split('\n')
  expect(lines.length).toBe(2)
  expect(JSON.parse(lines[1] ?? '')).toEqual({
    at: '1970-01-01T00:00:01.000Z',
    event: 'open.failed',
    argv: ['sn', 'open', 'incident', '-q', 'active=true'],
    exitCode: 1,
    stderr,
    stdout: '',
    version: 'sn 0.13.1',
    PATH: '/usr/bin',
  })
})

test('a piped call is read from its tee copy, which Output opens', async ($, on) => {
  mock.clock(on, { now: 1000 })
  mock.env(on, { HOME: '/home/me', PATH: '/usr/bin' })
  const files = new Map<string, string>()
  const ran: (readonly string[])[] = []
  const commands: string[] = []
  on('fs.exists', ($, e) => ({ value: files.has(e.path) }))
  on('fs.read', ($, e) => ({ value: files.get(e.path) ?? '' }))
  on('fs.stat', ($, e) => ({ value: { size: (files.get(e.path) ?? '').length } as never }))
  on('fs.write', ($, e) => {
    files.set(e.path, e.text)
    return { value: undefined }
  })
  on('process.run', ($, e) => {
    ran.push(e.argv)
    return {
      value: { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
    }
  })
  on('ui.open', () => ({ value: { isPlaced: true } }))
  // The shell: the tee writes sn's output, the model sees only jq's.
  on('tool.call', { tool: 'Bash' }, ($, e) => {
    commands.push(e.command)
    const tee = /\| tee '([^']+)' \|/.exec(e.command)?.[1]
    if (tee !== undefined) files.set(tee, '[{"number":"INC1"},{"number":"INC2"}]')
    return { result: { stdout: tee === undefined ? '{"sys_id":"abc","number":"INC9"}' : '2\n', stderr: '', interrupted: false } }
  })

  const piped = await $.tool.call({ tool: 'Bash', command: 'sn table list incident | jq length', tool_use_id: 'toolu_1' })
  await $.tool.call({ tool: 'Bash', command: 'sn table get incident abc', tool_use_id: 'toolu_2' })

  const path = '/home/me/.claude/sn-panel/outputs/toolu_1.out'
  expect(commands[0]).toBe(`sn table list incident | tee '${path}' | jq length`)
  expect(commands[1]).toBe('sn table get incident abc')
  // What the model gets back is the pipeline's output, untouched.
  expect((piped as { result: { stdout: string } }).result.stdout).toBe('2\n')
  expect(files.get('/home/me/.claude/sn-panel/outputs/toolu_2.out')).toBe('{"sys_id":"abc","number":"INC9"}')

  const ui = await $.ui.mount({ plugin: 'sn', surface: 'terminal', component: 'Pane', requestId: 'sn', props: PANE_PROPS })
  expect(await ui.find({ type: 'Text', text: /^ +2 records · INC1, INC2$/ })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: /piped/ })).toBeUndefined()
  expect(await ui.find({ type: 'Text', text: /3 records/ })).toBeDefined()
  await ui.press({ key: 'output-toolu_1' })
  expect(ran.at(-1)).toEqual(['code', path])

  await ui.press({ key: 'clear' })
  expect(ran.at(-1)).toEqual([
    'rm',
    '-f',
    path,
    '/home/me/.claude/sn-panel/outputs/toolu_1.js',
    '/home/me/.claude/sn-panel/outputs/toolu_2.out',
    '/home/me/.claude/sn-panel/outputs/toolu_2.js',
  ])
  await ui.unmount()
})

test('a script run folds its script and output under a toggle', async ($, on) => {
  mock.clock(on, { now: 1000 })
  mock.env(on, { HOME: '/home/me' })
  const files = new Map<string, string>([['/work/job.js', 'var n = 1;\ngs.info(n);\n']])
  on('fs.exists', ($, e) => ({ value: files.has(e.path) }))
  on('fs.read', ($, e) => ({ value: files.get(e.path) ?? '' }))
  on('fs.write', ($, e) => {
    files.set(e.path, e.text)
    return { value: undefined }
  })
  on('session.cwd', () => ({ value: '/work' }))
  on('process.run', () => ({
    value: { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
  }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  const commands: string[] = []
  on('tool.call', { tool: 'Bash' }, ($, e) => {
    commands.push(e.command)
    // The shell: a tee in front of sn copies what the pipe fed it.
    const copy = /\| tee '([^']+\.js)' \|/.exec(e.command)?.[1]
    if (copy !== undefined) files.set(copy, 'gs.info("piped in")')
    return {
      result: {
        stdout:
          '{"elapsed_ms":9,"error":null,"history_id":"h1","messages":["src: noted"],"ok":true,' +
          '"output":["one","two"],"rollback_context":null,"scope":"global"}',
        stderr: '',
        interrupted: false,
      },
    }
  })

  await $.tool.call({ tool: 'Bash', command: 'sn script run @job.js --yes', tool_use_id: 'toolu_f' })
  await $.tool.call({ tool: 'Bash', command: `printf x | sn script run @- --yes`, tool_use_id: 'toolu_p' })
  expect(commands[1]).toBe(`printf x | tee '/home/me/.claude/sn-panel/outputs/toolu_p.js' | sn script run @- --yes`)

  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ plugin: 'sn', surface, component: 'Pane', requestId: 'sn', props: PANE_PROPS })
    // Folded: the summary, no code.
    expect(await ui.find({ type: 'Button', text: /completed · one/ })).toBeDefined()
    // No Open or Output on a script run: the entry itself is the control.
    expect(await ui.find({ type: 'Button', key: 'output-toolu_f' })).toBeUndefined()
    expect(await ui.find({ type: 'Button', key: 'open-toolu_f' })).toBeUndefined()
    expect(await ui.find({ type: 'Text', text: /gs\.info\(n\)/ })).toBeUndefined()

    await ui.press({ key: 'toggle-toolu_f' })
    expect(await ui.find({ type: 'Text', text: 'script · job.js' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: 'gs.info(n);' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: 'two' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: '· src: noted' })).toBeDefined()

    // The summary row toggles too.
    await ui.press({ key: 'toggle-summary-toolu_p' })
    expect(await ui.find({ type: 'Text', text: 'script · stdin' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: 'gs.info("piped in")' })).toBeDefined()

    await ui.press({ key: 'toggle-toolu_f' })
    await ui.press({ key: 'toggle-toolu_p' })
    expect(await ui.find({ type: 'Text', text: 'gs.info(n);' })).toBeUndefined()
    await ui.unmount()
  }
})
