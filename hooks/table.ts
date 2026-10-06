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
  // The table, query or profile came from a shell expansion ($VAR, $(…), `…`).
  // Only the shell knows its value, so there is nothing truthful to open.
  isDynamic: boolean
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
// The flags of `sn table` and the globals that consume the next word. A flag
// missing here has its value read as a positional: `list -q active=true
// incident` would name a table called `active=true`.
const VALUE_LONGS = new Set([
  'profile',
  'output',
  'timeout',
  'proxy',
  'ca-cert',
  'proxy-ca-cert',
  'query',
  'sysparm-query',
  'fields',
  'sysparm-fields',
  'setlimit',
  'limit',
  'setLimit',
  'sysparm-limit',
  'page-size',
  'offset',
  'sysparm-offset',
  'display-value',
  'sysparm-display-value',
  'view',
  'sysparm-view',
  'query-category',
  'sysparm-query-category',
  'max-records',
  'paginate',
  'resume-from',
  'data',
  'field',
])
const VALUE_SHORTS = 'pqfDF'
const QUERY_FLAGS = new Set(['-q', '--query', '--sysparm-query'])
const PROFILE_FLAGS = new Set(['-p', '--profile'])
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

type Word = { text: string; isDynamic: boolean }

// Shell words with their quoting removed. A word is dynamic when the shell
// substitutes part of it before sn runs, so its text here is not its value.
function lex(text: string): Word[] {
  const out: Word[] = []
  let word = ''
  let hasWord = false
  let isDynamic = false
  let quote: string | null = null

  for (let i = 0; i < text.length; i++) {
    const c = text.charAt(i)
    const next = text.charAt(i + 1)
    if (quote === "'") {
      if (c === "'") quote = null
      else word += c
      continue
    }
    // A `$` ending the text is a `$(` that segments() split at.
    if (c === '`' || (c === '$' && (next === '' || /[\w{(@*#?!$-]/.test(next)))) isDynamic = true
    if (quote === '"') {
      if (c === '"') quote = null
      else if (c === '\\' && next === '\n') i++
      else if (c === '\\' && next !== '' && '$`"\\'.includes(next)) word += text.charAt(++i)
      else word += c
      continue
    }
    // A backslash-newline continues the line; it is not a word of its own.
    if (c === '\\' && next === '\n') {
      i++
    } else if (c === "'" || c === '"') {
      quote = c
      hasWord = true
    } else if (c === '\\' && next !== '') {
      word += text.charAt(++i)
      hasWord = true
    } else if (/\s/.test(c)) {
      if (hasWord) out.push({ text: word, isDynamic })
      word = ''
      hasWord = false
      isDynamic = false
    } else {
      word += c
      hasWord = true
    }
  }
  if (hasWord) out.push({ text: word, isDynamic })

  return out
}

type Arg =
  | { kind: 'positional'; word: Word; at: number }
  // `name` keeps its dashes; `width` is how many words the option spans.
  | { kind: 'option'; name: string; value: Word | null; at: number; width: number }

// Sorts argv into positionals and options the way clap reads it: `--flag
// value`, `--flag=value`, `-q value`, `-qvalue`, `-q=value`, a short cluster
// ending in a value flag (`-dq value`), and `--` ending the options.
function scan(w: readonly Word[], from: number): Arg[] {
  const out: Arg[] = []

  for (let i = from; i < w.length; i++) {
    const word = w[i]
    if (word === undefined) break
    const { text } = word
    if (text === '--') {
      for (let k = i + 1; k < w.length; k++) {
        const rest = w[k]
        if (rest !== undefined) out.push({ kind: 'positional', word: rest, at: k })
      }
      break
    }
    if (text.startsWith('--')) {
      const eq = text.indexOf('=')
      const next = w[i + 1]
      if (eq > 0) {
        const value = { text: text.slice(eq + 1), isDynamic: word.isDynamic }
        out.push({ kind: 'option', name: text.slice(0, eq), value, at: i, width: 1 })
      } else if (VALUE_LONGS.has(text.slice(2)) && next !== undefined) {
        out.push({ kind: 'option', name: text, value: next, at: i, width: 2 })
        i++
      } else {
        out.push({ kind: 'option', name: text, value: null, at: i, width: 1 })
      }
    } else if (text.startsWith('-') && text.length > 1) {
      for (let k = 1; k < text.length; k++) {
        const name = `-${text.charAt(k)}`
        if (!VALUE_SHORTS.includes(text.charAt(k))) {
          out.push({ kind: 'option', name, value: null, at: i, width: 1 })
          continue
        }
        const attached = text.slice(k + 1).replace(/^=/, '')
        const next = w[i + 1]
        if (attached !== '') {
          const value = { text: attached, isDynamic: word.isDynamic }
          out.push({ kind: 'option', name, value, at: i, width: 1 })
        } else if (next !== undefined) {
          out.push({ kind: 'option', name, value: next, at: i, width: 2 })
          i++
        } else {
          out.push({ kind: 'option', name, value: null, at: i, width: 1 })
        }
        break
      }
    } else {
      out.push({ kind: 'positional', word, at: i })
    }
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
  let isInBackticks = false

  for (const seg of segments(command)) {
    // A backtick that opens a substitution cuts the command short of its end.
    const isCutShort = seg.sep === '`' && !isInBackticks
    if (seg.sep === '`') isInBackticks = !isInBackticks

    const w = lex(seg.text)
    let i = 0
    while (i < w.length && (/^[A-Za-z_][A-Za-z0-9_]*=/.test(w[i]?.text ?? '') || LEADERS.has(w[i]?.text ?? ''))) {
      i++
    }
    const bin = w[i]?.text
    if (bin === undefined || (bin !== 'sn' && !bin.endsWith('/sn'))) continue

    const parsed = scan(w, i + 1)
    const positionals = parsed.filter(arg => arg.kind === 'positional')
    const options = parsed.filter(arg => arg.kind === 'option')
    const group = positionals[0]
    if (group?.word.text !== 'table') continue

    const named = positionals[1]
    const hasVerb = named !== undefined && VERBS.has(named.word.text)
    const target = positionals[hasVerb ? 2 : 1]
    const second = positionals[hasVerb ? 3 : 2]
    const targetText = target?.word.text ?? ''
    let verb = 'list'
    if (hasVerb) verb = named.word.text
    else if (targetText.includes(':') || second !== undefined) verb = 'get'

    const colon = targetText.indexOf(':')
    const table = target === undefined ? '?' : colon > 0 ? targetText.slice(0, colon) : targetText
    // Which part of a dynamic `table:$ID` word the shell fills in.
    const isExpanded = (word: Word | undefined, part: string) => word?.isDynamic === true && /[$`]/.test(part)
    let record: string | null = null
    if (colon > 0) record = targetText.slice(colon + 1)
    else if (verb === 'get' || verb === 'update' || verb === 'delete') record = second?.word.text ?? null
    if (record !== null && isExpanded(colon > 0 ? target?.word : second?.word, record)) record = null

    const query = verb === 'list' ? (options.find(opt => QUERY_FLAGS.has(opt.name))?.value ?? null) : null
    const profile = options.find(opt => PROFILE_FLAGS.has(opt.name))?.value ?? null

    const hidden = new Set<number>()
    if (hasVerb) hidden.add(named.at)
    if (target !== undefined) hidden.add(target.at)
    for (const opt of options) {
      if (PROFILE_FLAGS.has(opt.name)) for (let k = 0; k < opt.width; k++) hidden.add(opt.at + k)
    }
    const rest = w.filter((_, at) => at > group.at && !hidden.has(at)).map(word => word.text)
    if (colon > 0) rest.unshift(targetText.slice(colon + 1))

    return {
      bin: bin.startsWith('/') ? bin : 'sn',
      verb,
      table,
      record,
      query: query?.text ?? null,
      args: rest.map(quoteForDisplay).join(' '),
      profile: profile?.text ?? null,
      isPiped: seg.sep === '|',
      isDynamic:
        isCutShort || isExpanded(target?.word, table) || query?.isDynamic === true || profile?.isDynamic === true,
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
// there is nothing to show: a failure, a delete, a record the call never named,
// a table or query only the shell knew.
export function openArgv(call: TableInvocation, outcome: Outcome): string[] | null {
  if (outcome.status !== 'ok' || call.table === '?' || call.verb === 'delete' || call.isDynamic) return null

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
