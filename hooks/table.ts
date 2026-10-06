// Pure parsing of `sn table` invocations and of what they printed.

export type TableInvocation = {
  bin: string
  verb: string
  table: string
  // The record a get/update/delete named: a sys_id, or a number from `table:number`.
  record: string | null
  query: string | null
  args: string
  profile: string | null
  isPiped: boolean
}

export type Outcome = {
  status: 'ok' | 'failed'
  summary: string
  records: number | null
  exitCode: number | null
  // The sys_id of the one record the output held, when it held exactly one.
  sysId: string | null
}

const VERBS = new Set(['list', 'get', 'create', 'update', 'delete'])
const GLOBAL_VALUE_FLAGS = new Set([
  '-p',
  '--profile',
  '--output',
  '--timeout',
  '--proxy',
  '--ca-cert',
  '--proxy-ca-cert',
])
// Words that can sit in front of a command without being the command.
const LEADERS = new Set(['time', 'command', 'exec', 'env', 'nohup', 'then', 'do', 'else'])
const EXIT_MEANING: Record<number, string> = {
  1: 'usage',
  2: 'API',
  3: 'transport',
  4: 'auth',
  130: 'interrupted',
}

type Segment = { text: string; sep: string }

// Splits on unquoted ; & | ( ) ` and newlines, keeping the separator that
// ended each segment so a pipe after `sn` is visible. `2>&1` and `&>` are
// redirections, not separators.
export function segments(command: string): Segment[] {
  const out: Segment[] = []
  let quote: string | null = null
  let start = 0

  for (let i = 0; i < command.length; i++) {
    const c = command.charAt(i)
    if (quote !== null) {
      if (c === '\\' && quote === '"') i++
      else if (c === quote) quote = null
      continue
    }
    if (c === '\\') {
      i++
      continue
    }
    if (c === "'" || c === '"') {
      quote = c
      continue
    }
    if (c === '&' && (/[<>]/.test(command.charAt(i - 1)) || command.charAt(i + 1) === '>')) {
      continue
    }
    if (';&|()`\n'.includes(c)) {
      const at = i
      let sep = c
      if ((c === '|' || c === '&') && command.charAt(i + 1) === c) {
        sep = c + c
        i++
      }
      out.push({ text: command.slice(start, at), sep })
      start = i + 1
    }
  }
  out.push({ text: command.slice(start), sep: '' })

  return out
}

export function words(text: string): string[] {
  const out: string[] = []
  let word = ''
  let hasWord = false
  let quote: string | null = null

  for (let i = 0; i < text.length; i++) {
    const c = text.charAt(i)
    if (quote !== null) {
      if (c === quote) quote = null
      else if (c === '\\' && quote === '"' && i + 1 < text.length) word += text.charAt(++i)
      else word += c
      continue
    }
    if (c === "'" || c === '"') {
      quote = c
      hasWord = true
    } else if (c === '\\' && i + 1 < text.length) {
      word += text.charAt(++i)
      hasWord = true
    } else if (/\s/.test(c)) {
      if (hasWord) out.push(word)
      word = ''
      hasWord = false
    } else {
      word += c
      hasWord = true
    }
  }
  if (hasWord) out.push(word)

  return out
}

function profileOf(w: readonly string[]): string | null {
  for (let i = 0; i < w.length; i++) {
    const word = w[i] ?? ''
    if (word === '-p' || word === '--profile') return w[i + 1] ?? null
    if (word.startsWith('--profile=')) return word.slice('--profile='.length)
  }

  return null
}

function queryOf(w: readonly string[]): string | null {
  for (let i = 0; i < w.length; i++) {
    const word = w[i] ?? ''
    if (word === '-q' || word === '--query' || word === '--sysparm-query') return w[i + 1] ?? null
    for (const flag of ['--query=', '--sysparm-query=']) {
      if (word.startsWith(flag)) return word.slice(flag.length)
    }
  }

  return null
}

function withoutProfile(w: readonly string[]): string[] {
  const out: string[] = []
  for (let i = 0; i < w.length; i++) {
    const word = w[i] ?? ''
    if (word === '-p' || word === '--profile') i++
    else if (!word.startsWith('--profile=')) out.push(word)
  }

  return out
}

function quoteForDisplay(word: string): string {
  return /[\s^|&;]/.test(word) ? `"${word}"` : word
}

// The first `sn table ...` in a shell command, or null when it has none.
// Covers the implied verb too: `sn table incident` lists, `sn table
// incident <sys_id>` and `sn table incident:INC0010001` get.
export function parseSnTable(command: string): TableInvocation | null {
  for (const seg of segments(command)) {
    const w = words(seg.text)
    let i = 0
    while (i < w.length && (/^[A-Za-z_][A-Za-z0-9_]*=/.test(w[i] ?? '') || LEADERS.has(w[i] ?? ''))) {
      i++
    }
    const bin = w[i]
    if (bin === undefined || (bin !== 'sn' && !bin.endsWith('/sn'))) continue

    let j = i + 1
    const nextPositional = (): string | undefined => {
      while (j < w.length) {
        const word = w[j++] ?? ''
        if (!word.startsWith('-')) return word
        if (GLOBAL_VALUE_FLAGS.has(word)) j++
      }

      return undefined
    }
    if (nextPositional() !== 'table') continue

    const first = nextPositional()
    let verb = 'list'
    let target: string | undefined
    if (first !== undefined && VERBS.has(first)) {
      verb = first
      target = nextPositional()
    } else if (first !== undefined) {
      const after = w[j]
      target = first
      verb = first.includes(':') || (after !== undefined && !after.startsWith('-')) ? 'get' : 'list'
    }

    const rest = withoutProfile(w.slice(j))
    const colon = target?.indexOf(':') ?? -1
    const table = target === undefined ? '?' : colon > 0 ? target.slice(0, colon) : target
    let record: string | null = null
    if (target !== undefined && colon > 0) {
      record = target.slice(colon + 1)
      rest.unshift(record)
    } else if (verb === 'get' || verb === 'update' || verb === 'delete') {
      const next = w[j]
      record = next !== undefined && !next.startsWith('-') ? next : null
    }

    return {
      bin: bin.startsWith('/') ? bin : 'sn',
      verb,
      table,
      record,
      query: verb === 'list' ? queryOf(w) : null,
      args: rest.map(quoteForDisplay).join(' '),
      profile: profileOf(w),
      isPiped: seg.sep === '|',
    }
  }

  return null
}

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

function recordLabel(record: Record<string, unknown>): string | null {
  const nested = record.record
  const inner = nested !== null && typeof nested === 'object' ? (nested as Record<string, unknown>) : {}

  return (
    displayOf(record.number) ??
    displayOf(inner.number) ??
    displayOf(record.name) ??
    displayOf(record.sys_id)
  )
}

function describeStdout(out: string, verb: string): Pick<Outcome, 'summary' | 'records' | 'sysId'> {
  if (out === '') return { summary: 'no output', records: null, sysId: null }

  try {
    const value: unknown = JSON.parse(out)
    if (Array.isArray(value)) {
      return { summary: plural(value.length, 'record'), records: value.length, sysId: null }
    }
    if (value !== null && typeof value === 'object') {
      const record = value as Record<string, unknown>
      // A get is one record even when -f left sys_id out.
      if (verb === 'get' || 'sys_id' in record || 'record' in record) {
        const label = recordLabel(record)
        return {
          summary: label ? `1 record · ${label}` : '1 record',
          records: 1,
          sysId: displayOf(record.sys_id),
        }
      }
      const scalars = Object.entries(record)
        .map(([key, v]) => [key, displayOf(v)] as const)
        .filter(([, v]) => v !== null)
        .slice(0, 3)
        .map(([key, v]) => `${key}=${clip(v ?? '', 24)}`)
      return { summary: scalars.length > 0 ? scalars.join(' · ') : 'object', records: null, sysId: null }
    }
    return { summary: clip(String(value), 60), records: null, sysId: null }
  } catch {
    // Not one JSON document: JSONL (an --all walk) or text.
  }

  const lines = out.split('\n').filter(line => line.trim() !== '')
  const isJsonl = lines.every(line => {
    try {
      const v: unknown = JSON.parse(line)
      return v !== null && typeof v === 'object' && !Array.isArray(v)
    } catch {
      return false
    }
  })
  if (isJsonl) return { summary: plural(lines.length, 'record'), records: lines.length, sysId: null }

  return { summary: plural(lines.length, 'line'), records: null, sysId: null }
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
      displayOf(value.warning) ?? displayOf(value.message) ?? displayOf((value.error as { message?: unknown } | undefined)?.message)
    if (message !== null) return message
  } catch {
    // Plain text.
  }

  return line
}

export function summarizeOutput(stdout: string, stderr: string, isPiped: boolean, verb = 'list'): Outcome {
  const described = describeStdout(stdout.trim(), verb)
  const warning = firstLine(stderr)
  const parts = [isPiped ? `piped · ${described.summary}` : described.summary]
  if (warning !== null) parts.push(`⚠ ${clip(warning, 80)}`)

  return {
    status: 'ok',
    summary: parts.join(' · '),
    records: isPiped ? null : described.records,
    exitCode: 0,
    sysId: isPiped ? null : described.sysId,
  }
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

export function summarizeError(text: string): Outcome {
  const code = /^Exit code (\d+)/m.exec(text)
  const exitCode = code ? Number(code[1]) : null
  const meaning = exitCode === null ? undefined : EXIT_MEANING[exitCode]
  const head = exitCode === null ? 'error' : `exit ${exitCode}${meaning ? ` ${meaning}` : ''}`
  const detail = errorEnvelope(text) ?? firstLine(text.replace(/^Exit code \d+\s*/m, ''))

  return {
    status: 'failed',
    summary: detail === null ? head : `${head} · ${clip(detail, 120)}`,
    records: null,
    exitCode,
    sysId: null,
  }
}

// The `sn open` argv that shows what a call read or wrote in the browser: a
// list call's (filtered) list view, any other call's record form. Null when
// there is nothing to show: a failure, a delete, a record the call never named.
export function openArgv(call: TableInvocation, outcome: Outcome): string[] | null {
  if (outcome.status !== 'ok' || call.table === '?' || call.verb === 'delete') return null

  let target: string[]
  if (call.verb === 'list') {
    target = call.query === null ? [call.table] : [call.table, '-q', call.query]
  } else {
    const record = outcome.sysId ?? call.record
    if (record === null) return null
    target = [`${call.table}:${record}`]
  }

  return [call.bin, 'open', ...target, ...(call.profile === null ? [] : ['-p', call.profile])]
}

export function formatMs(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`
}
