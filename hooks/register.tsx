import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, ToolCallResult } from 'claude-code'

import type { SnPanelCall, SnPanelTotals } from '../types'
import { formatMs, openArgv, parseSnTable, summarizeError, summarizeOutput } from './table'
import type { Outcome, TableInvocation } from './table'

const PANE = 'sn-table'
const TITLE = 'sn table'
const KEEP = 50
const EMPTY: SnPanelTotals = { calls: 0, ok: 0, failed: 0, records: 0, byTable: {} }

const calls = atom({ plugin: 'sn', key: 'calls' } as const, [])
const totals = atom({ plugin: 'sn', key: 'totals' } as const, EMPTY)
const autoOpened = atom({ plugin: 'sn', key: 'autoOpened' } as const, false)

const LOG_NAME = 'sn-panel.log'
const LOG_KEEP = 200

// Where the panel's own failures are kept, beside Claude Code's other files.
// Null when the environment names no home to put it under.
async function logFile($: EngineInterface): Promise<string | null> {
  const configDir = await $.env.get('CLAUDE_CONFIG_DIR')
  if (configDir) return `${configDir}/${LOG_NAME}`
  const home = (await $.env.get('HOME')) ?? (await $.env.get('USERPROFILE'))

  return home ? `${home}/.claude/${LOG_NAME}` : null
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

const GLYPH = { running: '…', ok: '✓', failed: '✗' } as const
const COLOR = { running: 'yellow', ok: 'green', failed: 'red' } as const

function outcomeOf(ran: ToolCallResult<'Bash'>, invocation: TableInvocation): Outcome {
  if (ran.deny !== undefined) {
    return { status: 'failed', summary: `denied · ${ran.deny}`, records: null, exitCode: null, sysId: null }
  }
  if (ran.isError) {
    return summarizeError(ran.text ?? (typeof ran.result === 'string' ? ran.result : ''))
  }
  if (ran.result.interrupted) {
    return { status: 'failed', summary: 'interrupted', records: null, exitCode: null, sysId: null }
  }
  if (ran.result.backgroundTaskId !== undefined) {
    return { status: 'ok', summary: 'running in background', records: null, exitCode: null, sysId: null }
  }

  return summarizeOutput(ran.result.stdout, ran.result.stderr, invocation.isPiped, invocation.verb)
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'sn-panel',
      description: 'Show the sn table commands of this session in a side panel',
    })

    return next(e)
  })

  on('command.run', { command: 'sn-panel' }, async $ => {
    await $.ui.open({ id: PANE, title: TITLE })

    return { text: 'sn table panel opened.' }
  })

  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const invocation = parseSnTable(e.command)
    if (invocation === null) {
      return next(e)
    }

    const startedAt = await $.clock.now()
    const entry: SnPanelCall = {
      id: e.tool_use_id ?? `call-${startedAt}`,
      ...invocation,
      status: 'running',
      summary: 'running',
      records: null,
      ms: null,
      open: null,
    }
    await update($, calls, list => [...list, entry].slice(-KEEP))
    if (!(await read($, autoOpened))) {
      await update($, autoOpened, () => true)
      void $.ui.open({ id: PANE, title: TITLE }).catch(() => undefined)
    }

    const ran = await next(e)

    // Bookkeeping must never cost the model its tool result.
    try {
      const ms = Math.round((await $.clock.now()) - startedAt)
      const outcome = outcomeOf(ran, invocation)
      const open = openArgv(invocation, outcome)
      await update($, calls, list =>
        list.map(one =>
          one.id === entry.id
            ? { ...one, status: outcome.status, summary: outcome.summary, records: outcome.records, ms, open }
            : one,
        ),
      )
      await update($, totals, t => ({
        calls: t.calls + 1,
        ok: t.ok + (outcome.status === 'ok' ? 1 : 0),
        failed: t.failed + (outcome.status === 'failed' ? 1 : 0),
        records: t.records + (outcome.records ?? 0),
        byTable: { ...t.byTable, [invocation.table]: (t.byTable[invocation.table] ?? 0) + 1 },
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
    const { Box, Button, Text } = $.ui.resolve(e)
    const list = await read($, calls)
    const t = await read($, totals)
    const tables = Object.entries(t.byTable)
      .sort(([, a], [, b]) => b - a)
      .slice(0, 6)
      .map(([table, n]) => `${table} ${n}`)
      .join(' · ')

    // A failed Open is logged with what would explain it from outside the
    // session: the argv, the whole of stderr, and which sn answered to that name.
    const openInBrowser = async (call: SnPanelCall, argv: readonly string[]) => {
      const what = call.verb === 'list' ? `the ${call.table} list` : argv[2]
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
          argv,
          error: message,
          PATH: (await $.env.get('PATH')) ?? null,
        })
        $.ui.toast(`sn open could not start: ${message}${logged(path)}`)
      }
    }

    return (
      <Box flexDirection="column">
        <Box flexDirection="row" justifyContent="space-between">
          <Text bold wrap="truncate-end">
            {`${t.calls} calls · ${t.ok} ok · ${t.failed} failed · ${t.records} records`}
          </Text>
          <Button
            key="clear"
            label="Clear"
            hotkey="c"
            onPress={() => {
              void update($, calls, () => [])
              void update($, totals, () => EMPTY)
            }}
          />
        </Box>
        {tables !== '' && (
          <Text dimColor wrap="truncate-end">
            {tables}
          </Text>
        )}
        {list.length === 0 && <Text dimColor>No sn table commands yet.</Text>}
        {[...list].reverse().map(call => (
          <Box key={call.id} flexDirection="column" marginTop={1}>
            <Box flexDirection="row" justifyContent="space-between">
              <Text color={COLOR[call.status]} bold wrap="truncate-end">
                {`${GLYPH[call.status]} ${call.verb} ${call.table}${call.profile ? ` @${call.profile}` : ''}`}
              </Text>
              <Box flexDirection="row" gap={1}>
                <Text dimColor>{call.ms === null ? '' : formatMs(call.ms)}</Text>
                {Array.isArray(call.open) && (
                  <Button
                    key={`open-${call.id}`}
                    label="Open"
                    onPress={() => {
                      if (Array.isArray(call.open)) void openInBrowser(call, call.open)
                    }}
                  />
                )}
              </Box>
            </Box>
            {call.args !== '' && (
              <Text dimColor wrap="truncate-end">
                {`  ${call.args}`}
              </Text>
            )}
            <Text color={call.status === 'failed' ? 'red' : undefined} wrap="truncate-end">
              {`  ${call.summary}`}
            </Text>
          </Box>
        ))}
      </Box>
    )
  })
}
