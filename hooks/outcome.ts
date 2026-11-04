// What an `sn` call printed, summarized for its entry, and what its Open
// button shows. A command that prints something of its own shape gets a
// summarizer in SUMMARIZERS; every other one is read by its kind.

import { pastTense } from './kinds'
import type { Kind } from './kinds'
import type { SnInvocation } from './parse'

export type Outcome = {
  status: 'ok' | 'failed'
  summary: string
  // Lines shown under the summary, as printed: what a script logged.
  lines: string[]
  records: number | null
  exitCode: number | null
  // The sys_id of the one record the output held, when it held exactly one.
  sysId: string | null
}

const EXIT_MEANING: Record<number, string> = {
  1: 'usage',
  2: 'API',
  3: 'transport',
  4: 'auth',
  130: 'interrupted',
}
// A script's logged lines kept for its expanded view.
const MAX_LINES = 200

function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`
}

function clip(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text
}

function displayOf(value: unknown): string | null {
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return String(value)
  }
  if (value !== null && typeof value === 'object') {
    const pair = value as { display_value?: unknown; value?: unknown }
    return displayOf(pair.display_value) ?? displayOf(pair.value)
  }

  return null
}

function objectOf(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {}
}

// The record inside a response: itself, or `record` / `attributes` (cmdb) inside it.
function innerRecord(record: Record<string, unknown>): Record<string, unknown> {
  return { ...objectOf(record.attributes), ...objectOf(record.record), ...record }
}

function recordLabel(record: Record<string, unknown>): string | null {
  const inner = innerRecord(record)

  return displayOf(inner.number) ?? displayOf(inner.name) ?? displayOf(inner.sys_id)
}

type Described = Pick<Outcome, 'summary' | 'records' | 'sysId'> & { label: string | null }

// One JSON document, or the one a text opens with: pretty-printed output
// followed by something that is not JSON still reads as its document.
function parseJson(out: string): { value: unknown } | null {
  try {
    return { value: JSON.parse(out) }
  } catch {
    // Look for a leading document.
  }
  if (!/^[[{]/.test(out)) return null
  const lines = out.split('\n')
  for (let end = lines.length - 1; end > 0; end--) {
    if (!/^[\]}]/.test(lines[end] ?? '')) continue
    try {
      return { value: JSON.parse(lines.slice(0, end + 1).join('\n')) }
    } catch {
      // A shorter prefix, then.
    }
  }

  return null
}

// The first few records' labels, which say more than how many there were.
function labelsOf(records: readonly unknown[], total: number): string {
  const labels = records
    .slice(0, 3)
    .map(one => (one !== null && typeof one === 'object' ? recordLabel(one as Record<string, unknown>) : displayOf(one)))
    .filter((label): label is string => label !== null)
    .map(label => clip(label, 30))
  if (labels.length === 0) return plural(total, 'record')

  return `${plural(total, 'record')} · ${labels.join(', ')}${total > labels.length ? ', …' : ''}`
}

// `key=value` for the scalars of an object, looking one level into nested
// objects (`{"stats":{"count":"44"}}` reads `count=44`).
function scalarsOf(record: Record<string, unknown>): string[] {
  const out: string[] = []
  for (const [key, v] of Object.entries(record)) {
    const shown = displayOf(v)
    if (shown !== null && (typeof v !== 'object' || v === null || 'value' in (v as object))) {
      out.push(`${key}=${clip(shown, 24)}`)
    } else if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      for (const [inner, w] of Object.entries(v as Record<string, unknown>)) {
        const value = typeof w === 'object' && w !== null && !('value' in w) ? null : displayOf(w)
        if (value !== null) out.push(`${inner}=${clip(value, 24)}`)
      }
    } else if (Array.isArray(v)) {
      out.push(`${key}=${plural(v.length, 'item')}`)
    }
  }

  return out.slice(0, 4)
}

function describeStdout(out: string, isOneRecord: boolean): Described {
  if (out === '') return { summary: 'no output', records: null, sysId: null, label: null }

  const parsed = parseJson(out)
  if (parsed !== null) {
    const { value } = parsed
    if (Array.isArray(value)) {
      return { summary: labelsOf(value, value.length), records: value.length, sysId: null, label: null }
    }
    if (value !== null && typeof value === 'object') {
      const record = value as Record<string, unknown>
      // A get is one record even when -f left sys_id out.
      if (isOneRecord || 'sys_id' in record || 'record' in record || 'attributes' in record) {
        const label = recordLabel(record)
        return {
          summary: label ? `1 record · ${label}` : '1 record',
          records: 1,
          sysId: displayOf(innerRecord(record).sys_id),
          label,
        }
      }
      const scalars = scalarsOf(record)
      return { summary: scalars.length > 0 ? scalars.join(' · ') : 'object', records: null, sysId: null, label: null }
    }
    return { summary: clip(String(value), 100), records: null, sysId: null, label: null }
  }

  // JSONL (an --all walk, a watch), or text.
  const lines = out.split('\n').filter(line => line.trim() !== '')
  const objects = lines.map(line => {
    try {
      const v: unknown = JSON.parse(line)
      return v !== null && typeof v === 'object' && !Array.isArray(v) ? v : null
    } catch {
      return null
    }
  })
  if (objects.every(one => one !== null)) {
    return { summary: labelsOf(objects, lines.length), records: lines.length, sysId: null, label: null }
  }

  // Text says what it is in its first line: a help page's about, a count.
  return { summary: clip((lines[0] ?? '').trim(), 100), records: null, sysId: null, label: null }
}

function firstLine(text: string): string | null {
  const line = text
    .split('\n')
    .map(one => one.trim())
    .find(one => one !== '')
  if (line === undefined) return null

  try {
    const value = JSON.parse(line) as Record<string, unknown>
    const message =
      displayOf(value.warning) ??
      displayOf(value.message) ??
      displayOf((value.error as { message?: unknown } | undefined)?.message)
    if (message !== null) return message
  } catch {
    // Plain text.
  }

  return line
}

function errorEnvelope(text: string): string | null {
  const starts = /\{\s*"error"\s*:/g
  for (let match = starts.exec(text); match !== null; match = starts.exec(text)) {
    const end = text.indexOf('\n', match.index)
    const candidates = [text.slice(match.index, end < 0 ? undefined : end), text.slice(match.index)]
    for (const candidate of candidates) {
      try {
        const { error } = JSON.parse(candidate) as { error?: { message?: unknown; status_code?: unknown } }
        const message = displayOf(error?.message)
        if (message !== null) {
          const status = displayOf(error?.status_code)
          return status === null ? message : `${message} (${status})`
        }
      } catch {
        // Try the next shape.
      }
    }
  }

  return null
}

function exitHead(exitCode: number | null): string {
  if (exitCode === null) return 'error'
  const meaning = EXIT_MEANING[exitCode]

  return `exit ${exitCode}${meaning ? ` ${meaning}` : ''}`
}

function exitCodeOf(text: string): number | null {
  const code = /^Exit code (\d+)/m.exec(text)

  return code ? Number(code[1]) : null
}

// ---- Commands that print a shape of their own ----

type ScriptResult = {
  ok: boolean
  scope: string
  output: string[]
  messages: string[]
  error: { type?: string; line?: number | null; message?: string } | null
  elapsed_ms: number
  history_id: string | null
}

// The result object `sn script run` prints, wherever it sits in the text: on
// its own as stdout, or in front of the stderr envelope in a failure.
function scriptResult(text: string): ScriptResult | null {
  for (const line of text.split('\n')) {
    if (!line.trimStart().startsWith('{')) continue
    try {
      const value = JSON.parse(line) as Partial<ScriptResult>
      if (typeof value.ok === 'boolean' && Array.isArray(value.output)) {
        return {
          ok: value.ok,
          scope: value.scope ?? 'global',
          output: value.output,
          messages: value.messages ?? [],
          error: value.error ?? null,
          elapsed_ms: value.elapsed_ms ?? 0,
          history_id: value.history_id ?? null,
        }
      }
    } catch {
      // Not this line.
    }
  }

  return null
}

function describeScript(result: ScriptResult, exitCode: number | null): Outcome {
  const lines = [...result.output, ...result.messages.map(message => `· ${message}`)].slice(0, MAX_LINES)
  const scope = result.scope === 'global' ? '' : ` · ${result.scope}`
  if (result.ok) {
    return {
      status: 'ok',
      summary:
        result.output.length === 0
          ? `completed, printed nothing${scope}`
          : `completed${scope} · ${clip(result.output[0] ?? '', 80)}`,
      lines,
      records: null,
      exitCode: 0,
      sysId: result.history_id,
    }
  }
  const { type, line, message } = result.error ?? {}
  const where = typeof line === 'number' ? ` line ${line}` : ''

  return {
    status: 'failed',
    summary: `${exitHead(exitCode)} · ${type ?? 'script'} error${where} · ${clip(message ?? 'failed', 120)}${scope}`,
    lines,
    records: null,
    exitCode,
    sysId: result.history_id,
  }
}

type Summarizer = {
  ok?: (stdout: string, stderr: string) => Outcome | null
  failed?: (text: string, exitCode: number | null) => Outcome | null
}

const SUMMARIZERS: Record<string, Summarizer> = {
  'script run': {
    ok: stdout => {
      const result = scriptResult(stdout)
      return result === null ? null : describeScript(result, 0)
    },
    failed: (text, exitCode) => {
      const result = scriptResult(text)
      return result === null ? null : describeScript(result, exitCode)
    },
  },
}

// ---- Every other command, by kind ----

export function summarizeOutput(stdout: string, stderr: string, call: SnInvocation, kind: Kind): Outcome {
  const own = SUMMARIZERS[call.command]?.ok?.(stdout, stderr)
  if (own != null) return own

  const verb = call.path[call.path.length - 1] ?? ''
  const isOneRecord = verb === 'get' || kind === 'write'
  const described = describeStdout(stdout.trim(), isOneRecord)
  let summary = described.summary
  if (kind === 'write' || kind === 'delete') {
    const past = pastTense(verb)
    const what = described.label ?? (call.record === null ? null : `${call.table ?? ''}:${call.record}`)
    if (past !== null) summary = what === null ? past : `${past} · ${what}`
  } else if (kind === 'stream' && described.records !== null) {
    summary = summary.replace(/^\d+ records?/, plural(described.records, 'event'))
  }

  const warning = firstLine(stderr)
  const parts = [call.isPiped ? `piped · ${summary}` : summary]
  if (warning !== null) parts.push(`⚠ ${clip(warning, 80)}`)
  const counts = kind === 'read' && !call.isPiped

  return {
    status: 'ok',
    summary: parts.join(' · '),
    lines: [],
    records: counts ? described.records : null,
    exitCode: 0,
    sysId: call.isPiped ? null : described.sysId,
  }
}

// `text` is the Bash tool's error text: `Exit code N`, then what was printed.
export function summarizeError(text: string, command = ''): Outcome {
  const exitCode = exitCodeOf(text)
  const own = SUMMARIZERS[command]?.failed?.(text, exitCode)
  if (own != null) return own

  const detail = errorEnvelope(text) ?? firstLine(text.replace(/^Exit code \d+\s*/m, ''))
  const head = exitHead(exitCode)

  return {
    status: 'failed',
    summary: detail === null ? head : `${head} · ${clip(detail, 120)}`,
    lines: [],
    records: null,
    exitCode,
    sysId: null,
  }
}

// A piped call, read from the copy a `tee` kept of what sn itself printed. The
// pipeline's exit status is its last command's, so sn's own failure shows only
// as the error envelope (on stderr, or in the copy under `2>&1`).
export function summarizeCaptured(captured: string, stderr: string, call: SnInvocation, kind: Kind): Outcome {
  const both = `${captured}\n${stderr}`
  if (errorEnvelope(both) !== null) return summarizeError(both, call.command)

  return summarizeOutput(captured, stderr, { ...call, isPiped: false }, kind)
}

// ---- The Open button ----

// Commands whose table and query make a list view.
const LIST_VIEW = new Set(['table list', 'cmdb list', 'watch', 'aggregate', 'gr'])
// Commands whose table and record make a record form.
const RECORD_FORM = new Set([
  'get',
  'table get',
  'table create',
  'table update',
  'cmdb get',
  'cmdb create',
  'cmdb update',
  'journal',
  'variables get',
  'variables set',
])
// Commands whose record lives in a table they do not name.
const FORM_TABLE: Record<string, string> = {
  'attachment get': 'sys_attachment',
  'attachment upload': 'sys_attachment',
  'change create': 'change_request',
  'change get': 'change_request',
  'change update': 'change_request',
  'script run': 'sys_script_execution_history',
}
// A failed script still has its execution history row, which holds the error.
const OPENS_ON_FAILURE = new Set(['script run'])

// The `sn open` argv that shows what a call read or wrote in the browser: a
// list call's (filtered) list view, a record call's form. Null when there is
// nothing to show: a failure, a delete, a record the call never named, a
// table or query only the shell knew.
export function openArgv(call: SnInvocation, outcome: Outcome, kind: Kind): string[] | null {
  if (call.isDynamic || kind === 'delete') return null
  if (outcome.status !== 'ok' && !OPENS_ON_FAILURE.has(call.command)) return null

  let target: string[]
  if (LIST_VIEW.has(call.command)) {
    if (call.table === null || call.table === '') return null
    target = call.query === null ? [call.table] : [call.table, '-q', call.query]
  } else {
    const table = FORM_TABLE[call.command] ?? (RECORD_FORM.has(call.command) ? call.table : null)
    const record = outcome.sysId ?? call.record
    if (table === null || table === '' || record === null) return null
    target = [`${table}:${record}`]
  }

  return [call.bin, 'open', ...target, ...(call.profile === null ? [] : ['-p', call.profile])]
}

export function formatMs(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`
}
