import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, ToolCallResult } from 'claude-code'

import type { SnPanelKind, SnPanelRun, SnPanelStats } from '../types'
import { KINDS, LOOK, kindOf } from './kinds'
import type { Kind } from './kinds'
import { openArgv, summarizeCaptured, summarizeError, summarizeOutput } from './outcome'
import type { Outcome } from './outcome'
import { parseSn, teeCommand } from './parse'
import type { SnInvocation } from './parse'
import { Entry } from './views'

const PANE = 'sn'
const TITLE = 'sn'
const KEEP = 50
const EMPTY: SnPanelStats = { calls: 0, ok: 0, failed: 0, records: 0, byKind: {}, byCommand: {} }

const runs = atom({ plugin: 'sn', key: 'entries' } as const, [])
const stats = atom({ plugin: 'sn', key: 'stats' } as const, EMPTY)
const filter = atom({ plugin: 'sn', key: 'filter' } as const, null)
const expanded = atom({ plugin: 'sn', key: 'expanded' } as const, [])

const LOG_NAME = 'sn-panel.log'
const LOG_KEEP = 200
// What each call printed, one file per entry, kept out of the model's context.
const OUTPUTS = 'sn-panel/outputs'
const OUTPUT_DAYS = 7
// Larger saved outputs are not read back to summarize.
const READ_LIMIT = 4 * 1024 * 1024

// Claude Code's config dir, where the panel keeps its files. Null when the
// environment names no home to put it under.
async function configDir($: EngineInterface): Promise<string | null> {
  const dir = await $.env.get('CLAUDE_CONFIG_DIR')
  if (dir) return dir
  const home = (await $.env.get('HOME')) ?? (await $.env.get('USERPROFILE'))

  return home ? `${home}/.claude` : null
}

// Where the panel's own failures are kept, beside Claude Code's other files.
async function logFile($: EngineInterface): Promise<string | null> {
  const dir = await configDir($)

  return dir === null ? null : `${dir}/${LOG_NAME}`
}

let outputsReady: Promise<string | null> | null = null

// The outputs folder, made once per load; null when it cannot be.
function outputsDir($: EngineInterface): Promise<string | null> {
  outputsReady ??= (async () => {
    const dir = await configDir($)
    if (dir === null) return null
    const path = `${dir}/${OUTPUTS}`
    const made = await $.process.run(['mkdir', '-p', path])

    return made.exitCode === 0 ? path : null
  })().catch(() => null)

  return outputsReady
}

function fileName(id: string): string {
  return `${id.replace(/[^A-Za-z0-9_-]/g, '_')}.out`
}

const SCRIPT_LIMIT = 64 * 1024

type ScriptSource = { text: string | null; from: string; stdinCopy: string | null }

// Where a `sn script run` call's script comes from, and its text when the hook
// can know it: inline, a heredoc, a file (read now, as it is when it runs), or
// a pipe into `@-` (copied by a tee, read after the run).
async function scriptSource(
  $: EngineInterface,
  call: SnInvocation,
  saved: string | null,
  isBackground: boolean,
): Promise<ScriptSource | null> {
  const operand = call.operands.script
  if (call.command !== 'script run' || operand === undefined) return null
  if (!operand.startsWith('@')) return { text: operand, from: 'inline', stdinCopy: null }
  if (operand === '@-') {
    if (call.heredoc !== null) return { text: call.heredoc, from: 'heredoc', stdinCopy: null }
    const canCopy = call.stdinPipeAt !== null && saved !== null && !isBackground
    return { text: null, from: 'stdin', stdinCopy: canCopy ? saved.replace(/\.out$/, '.js') : null }
  }

  let path = operand.slice(1)
  if (path.startsWith('~/')) path = `${(await $.env.get('HOME')) ?? '~'}${path.slice(1)}`
  else if (!path.startsWith('/')) path = `${await $.session.cwd()}/${path}`
  try {
    return { text: (await $.fs.read(path)).slice(0, SCRIPT_LIMIT), from: operand.slice(1), stdinCopy: null }
  } catch {
    return { text: null, from: operand.slice(1), stdinCopy: null }
  }
}

// Removes saved outputs whose entries are gone. Never fails its caller.
async function forget($: EngineInterface, paths: readonly (string | null | undefined)[]): Promise<void> {
  const files = paths
    .filter((path): path is string => typeof path === 'string')
    .flatMap(path => [path, path.replace(/\.out$/, '.js')])
  if (files.length === 0) return
  await $.process.run(['rm', '-f', ...files]).catch(() => undefined)
}

// Records a failure of the panel itself, never of the sn call it watched: one
// JSON object per line, newest last, the last LOG_KEEP kept. Answers the file
// it wrote to, for the toast. Logging must never fail its caller.
async function logFailure(
  $: EngineInterface,
  event: string,
  detail: Record<string, unknown>,
): Promise<string | null> {
  try {
    const line = JSON.stringify({ at: new Date(await $.clock.now()).toISOString(), event, ...detail })
    $.ui.log(`sn panel: ${line}`, { to: 'debug' })
    const path = await logFile($)
    if (path === null) return null
    const before = (await $.fs.exists(path)) ? await $.fs.read(path) : ''
    const lines = [...before.split('\n').filter(one => one !== ''), line].slice(-LOG_KEEP)
    await $.fs.write(path, `${lines.join('\n')}\n`)

    return path
  } catch {
    return null
  }
}

// What a running entry says, by what it is doing.
const RUNNING: Record<Kind, string> = {
  read: 'reading…',
  write: 'writing…',
  delete: 'deleting…',
  run: 'running…',
  stream: 'streaming…',
  session: 'working…',
  skill: 'loading…',
}

function outcomeOf(ran: ToolCallResult<'Bash'>, call: SnInvocation, kind: Kind): Outcome {
  const none = { lines: [], records: null, exitCode: null, sysId: null }
  if (ran.deny !== undefined) return { status: 'failed', summary: `denied · ${ran.deny}`, ...none }
  if (ran.isError) {
    return summarizeError(ran.text ?? (typeof ran.result === 'string' ? ran.result : ''), call.command)
  }
  if (ran.result.interrupted) return { status: 'failed', summary: 'interrupted', ...none }
  if (ran.result.backgroundTaskId !== undefined) {
    return { status: 'ok', summary: kind === 'stream' ? 'streaming in background' : 'running in background', ...none }
  }

  return summarizeOutput(ran.result.stdout, ran.result.stderr, call, kind)
}

function isSnSkill(skill: string): boolean {
  return skill === 'sn' || skill.endsWith(':sn')
}

async function addRun($: EngineInterface, run: SnPanelRun): Promise<void> {
  const before = await read($, runs)
  const kept = [...before, run].slice(-KEEP)
  await update($, runs, () => kept)
  const evicted = before.filter(one => !kept.some(k => k.id === one.id))
  void forget($, evicted.map(one => one.saved))
  // Every sn call brings the pane back, even after it was closed.
  void $.ui.open({ id: PANE, title: TITLE }).catch(() => undefined)
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'sn-panel',
      description: 'Show the sn commands of this session in a side panel',
    })
    // Outputs whose sessions are long gone.
    const dir = await outputsDir($)
    if (dir !== null) {
      void $.process
        .run(['find', dir, '-name', '*.out', '-mtime', `+${OUTPUT_DAYS}`, '-delete'])
        .catch(() => undefined)
    }

    return next(e)
  })

  on('command.run', { command: 'sn-panel' }, async $ => {
    await $.ui.open({ id: PANE, title: TITLE })

    return { text: 'sn panel opened.' }
  })

  on('skill.prompt', async ($, e, next) => {
    if (!isSnSkill(e.skill)) return next(e)

    try {
      const now = await $.clock.now()
      await addRun($, {
        id: `skill-${now}`,
        bin: 'sn',
        command: `skill ${e.skill}`,
        kind: 'skill',
        table: null,
        record: null,
        query: null,
        args: '',
        profile: null,
        isPiped: false,
        isDynamic: false,
        status: 'ok',
        summary: 'loaded into context',
        lines: [],
        records: null,
        ms: null,
        open: null,
        saved: null,
        script: null,
        scriptFrom: null,
      })
      await update($, stats, s => ({ ...s, byKind: { ...s.byKind, skill: (s.byKind.skill ?? 0) + 1 } }))
    } catch (error) {
      await logFailure($, 'skill.failed', { error: error instanceof Error ? error.message : String(error) })
    }

    return next(e)
  })

  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const call = parseSn(e.command)
    if (call === null) return next(e)

    const kind = kindOf(call)
    const startedAt = await $.clock.now()
    const id = e.tool_use_id ?? `call-${startedAt}`
    const dir = await outputsDir($)
    const saved = dir === null ? null : `${dir}/${fileName(id)}`
    // A piped call's output never reaches the hook: a tee between sn and the
    // pipe keeps a copy. A background call finishes after the hook, so it has none.
    const teeAt = saved !== null && e.run_in_background !== true ? call.pipeAt : null
    const source = await scriptSource($, call, saved, e.run_in_background === true).catch(() => null)
    const entry: SnPanelRun = {
      id,
      bin: call.bin,
      command: call.command,
      kind,
      table: call.table,
      record: call.record,
      query: call.query,
      args: call.args,
      profile: call.profile,
      isPiped: call.isPiped,
      isDynamic: call.isDynamic,
      status: 'running',
      summary: RUNNING[kind],
      lines: [],
      records: null,
      ms: null,
      open: null,
      saved: null,
      script: source?.text ?? null,
      scriptFrom: source?.from ?? null,
    }
    await addRun($, entry)

    // The later tee goes in first, so the earlier one's offset still holds.
    let command = e.command
    if (teeAt !== null && saved !== null) command = teeCommand(command, teeAt, saved)
    if (source?.stdinCopy != null && call.stdinPipeAt !== null) {
      command = teeCommand(command, call.stdinPipeAt, source.stdinCopy)
    }
    const ran = await next(command === e.command ? e : { ...e, command })

    // Bookkeeping must never cost the model its tool result.
    try {
      const ms = Math.round((await $.clock.now()) - startedAt)
      let outcome = outcomeOf(ran, call, kind)
      let kept: string | null = null
      let script = entry.script
      if (source?.stdinCopy != null && (await $.fs.exists(source.stdinCopy))) {
        script = (await $.fs.read(source.stdinCopy)).slice(0, SCRIPT_LIMIT)
      }
      if (teeAt !== null && saved !== null && (await $.fs.exists(saved))) {
        kept = saved
        const { size } = await $.fs.stat(saved)
        if (size <= READ_LIMIT && ran.deny === undefined) {
          // A failed pipeline's text holds sn's stderr too.
          const stderr = ran.isError ? (ran.text ?? '') : ran.result.stderr
          outcome = summarizeCaptured(await $.fs.read(saved), stderr, call, kind)
        }
      } else if (saved !== null && !call.isPiped && ran.deny === undefined) {
        // The hook already holds what an unpiped call printed.
        const text = ran.isError
          ? (ran.text ?? (typeof ran.result === 'string' ? ran.result : ''))
          : ran.result.stdout + (ran.result.stderr === '' ? '' : `\n${ran.result.stderr}`)
        if (text.trim() !== '') {
          await $.fs.write(saved, text)
          kept = saved
        }
      }
      const open = openArgv(call, outcome, kind)
      await update($, runs, list =>
        list.map(one =>
          one.id === entry.id
            ? {
                ...one,
                status: outcome.status,
                summary: outcome.summary,
                lines: outcome.lines,
                records: outcome.records,
                ms,
                open,
                saved: kept,
                script,
              }
            : one,
        ),
      )
      await update($, stats, s => ({
        calls: s.calls + 1,
        ok: s.ok + (outcome.status === 'ok' ? 1 : 0),
        failed: s.failed + (outcome.status === 'failed' ? 1 : 0),
        records: s.records + (outcome.records ?? 0),
        byKind: { ...s.byKind, [kind]: (s.byKind[kind] ?? 0) + 1 },
        byCommand: { ...s.byCommand, [call.command]: (s.byCommand[call.command] ?? 0) + 1 },
      }))
    } catch (error) {
      // Leave the entry as it stood.
      await logFailure($, 'bookkeeping.failed', {
        command: e.command,
        error: error instanceof Error ? (error.stack ?? error.message) : String(error),
      })
    }

    return ran
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const ui = $.ui.resolve(e)
    const { Box, Button, Text } = ui
    const list = await read($, runs)
    const open = await read($, expanded)
    const s = await read($, stats)
    const only = await read($, filter)
    const commands = Object.entries(s.byCommand)
      .sort(([, a], [, b]) => b - a)
      .slice(0, 6)
      .map(([command, n]) => `${command} ${n}`)
      .join(' · ')
    const shown = only === null ? list : list.filter(run => run.kind === only)

    // A failed Open is logged with what would explain it from outside the
    // session: the argv, the whole of stderr, and which sn answered to that name.
    // A saved output opens in VS Code, else the system's text editor.
    const viewOutput = async (path: string) => {
      for (const argv of [['code', path], ['open', '-t', path]]) {
        const ran = await $.process.run(argv).catch(() => null)
        if (ran?.exitCode === 0) return
      }
      const logged = await logFailure($, 'output.unopenable', { path, PATH: (await $.env.get('PATH')) ?? null })
      $.ui.toast(`Could not open ${path}${logged === null ? '' : ` · logged to ${logged}`}`)
    }

    const openInBrowser = async (run: SnPanelRun, argv: readonly string[]) => {
      const what = argv.includes('-q') || !(argv[2] ?? '').includes(':') ? `the ${argv[2]} list` : argv[2]
      const logged = (path: string | null) => (path === null ? '' : ` · logged to ${path}`)
      try {
        const ran = await $.process.run(argv)
        if (ran.exitCode === 0) {
          $.ui.toast(`Opened ${what} in the browser`)
          return
        }
        const version = await $.process.run([argv[0] ?? 'sn', '--version']).then(
          v => v.stdout.trim(),
          () => null,
        )
        const path = await logFailure($, 'open.failed', {
          argv,
          exitCode: ran.exitCode,
          stderr: ran.stderr,
          stdout: ran.stdout,
          version,
          PATH: (await $.env.get('PATH')) ?? null,
        })
        const { summary } = summarizeError(`Exit code ${ran.exitCode}\n${ran.stderr}`)
        $.ui.toast(`sn open: ${summary}${logged(path)}`)
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error)
        const path = await logFailure($, 'open.unstartable', {
          command: run.command,
          argv,
          error: message,
          PATH: (await $.env.get('PATH')) ?? null,
        })
        $.ui.toast(`sn open could not start: ${message}${logged(path)}`)
      }
    }

    const chip = (kind: SnPanelKind, n: number) => {
      const isActive = only === kind
      const label = `${LOOK[kind].glyph} ${kind} ${n}`
      return (
        <Button
          key={`kind-${kind}`}
          label={isActive ? `[${label}]` : label}
          hotkey={String(KINDS.indexOf(kind) + 1)}
          plain
          dimColor={n === 0 && !isActive}
          onPress={() => {
            void update($, filter, current => (current === kind ? null : kind))
          }}
        />
      )
    }

    return (
      <Box flexDirection="column">
        <Box flexDirection="row" justifyContent="space-between">
          <Text bold wrap="truncate-end">
            {`${s.calls} calls · ${s.ok} ok · ${s.failed} failed · ${s.records} records`}
          </Text>
          <Button
            key="clear"
            label="Clear"
            hotkey="c"
            onPress={() => {
              void forget($, list.map(one => one.saved))
              void update($, runs, () => [])
              void update($, stats, () => EMPTY)
              void update($, filter, () => null)
              void update($, expanded, () => [])
            }}
          />
        </Box>
        <Box flexDirection="row" flexWrap="wrap" columnGap={2}>
          {KINDS.map(kind => chip(kind, s.byKind[kind] ?? 0))}
        </Box>
        {commands !== '' && (
          <Text dimColor wrap="truncate-end">
            {commands}
          </Text>
        )}
        {shown.length === 0 && <Text dimColor>{only === null ? 'No sn commands yet.' : `No ${only} commands yet.`}</Text>}
        {[...shown].reverse().map(run => (
          <Entry
            key={run.id}
            ui={ui}
            run={run}
            isExpanded={open.includes(run.id)}
            onToggle={() => {
              void update($, expanded, ids => (ids.includes(run.id) ? ids.filter(id => id !== run.id) : [...ids, run.id]))
            }}
            onOpen={() => {
              if (Array.isArray(run.open)) void openInBrowser(run, run.open)
            }}
            onOutput={() => {
              if (typeof run.saved === 'string') void viewOutput(run.saved)
            }}
          />
        ))}
      </Box>
    )
  })
}
