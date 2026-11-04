// Pure parsing of `sn …` invocations out of a shell command. Which words are
// subcommands, which flags consume the next word and what each positional is
// called all come from `catalog.ts`, generated from `sn introspect`, so a new
// command or flag needs no change here.

import { CATALOG, GLOBAL_SHORTS, GLOBAL_VALUES } from './catalog'
import type { CatalogNode } from './catalog'

export type SnInvocation = {
  bin: string
  // The subcommand path, implied verb included: ['table', 'list'], ['change', 'task', 'get'].
  path: string[]
  // The path joined with spaces, or 'sn' for a bare `sn --version`.
  command: string
  // The table the call is about: a `table`, `class` or `staging_table` positional,
  // or the table half of a `table:ref` reference.
  table: string | null
  // The record it named: a sys_id positional, or the half after the colon.
  record: string | null
  query: string | null
  // The positionals by name, as typed.
  operands: Record<string, string>
  // The words after the path, less the table and the profile, for display.
  args: string
  profile: string | null
  isPiped: boolean
  // Where the `|` after sn sits in the command, when sn's stdout alone is
  // piped (not `|&`): the place a `tee` can copy what sn printed.
  pipeAt: number | null
  // Where the `|` feeding sn's stdin sits (`printf … | sn script run @-`).
  stdinPipeAt: number | null
  // A heredoc's body fed to sn's stdin (`sn script run @- <<'EOF'`).
  heredoc: string | null
  // The table, record, query or profile came from a shell expansion ($VAR,
  // $(…), `…`). Only the shell knows its value, so there is nothing truthful
  // to open.
  isDynamic: boolean
}

// Groups whose read verb may be left out: `sn table incident` lists.
const IMPLIED_VERB_GROUPS = new Set(['table', 'cmdb'])
const TABLE_OPERANDS = new Set(['table', 'class', 'staging_table'])
const RECORD_OPERANDS = new Set(['sys_id', 'task_sys_id', 'record'])
const QUERY_FLAGS = new Set(['-q', '--query', '--sysparm-query'])
const PROFILE_FLAGS = new Set(['-p', '--profile'])
// Words that can sit in front of a command without being the command.
const LEADERS = new Set(['time', 'command', 'exec', 'env', 'nohup', 'then', 'do', 'else'])

// `end` is where the separator sits in the whole command.
type Segment = { text: string; sep: string; end: number }

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
      out.push({ text: command.slice(start, at), sep, end: at })
      start = i + 1
    }
  }
  out.push({ text: command.slice(start), sep: '', end: command.length })

  return out
}

type Word = { text: string; isDynamic: boolean }

const REDIRECT = /^(?:\d+|&)?(?:>>|>&|<&|>\||<<|>|<)/

// Shell words with their quoting removed, redirections (`2>&1`, `> out.json`)
// left out: they are the shell's, not sn's arguments. A word is dynamic when
// the shell substitutes part of it before sn runs, so its text here is not
// its value.
function lex(text: string): Word[] {
  const out: (Word & { isRedirect: boolean })[] = []
  let word = ''
  let hasWord = false
  let isDynamic = false
  let isRedirect = false
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
    if (!hasWord && REDIRECT.test(text.slice(i))) isRedirect = true
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
      if (hasWord) out.push({ text: word, isDynamic, isRedirect })
      word = ''
      hasWord = false
      isDynamic = false
      isRedirect = false
    } else {
      word += c
      hasWord = true
    }
  }
  if (hasWord) out.push({ text: word, isDynamic, isRedirect })

  const words: Word[] = []
  for (let k = 0; k < out.length; k++) {
    const one = out[k]
    if (one === undefined) continue
    if (!one.isRedirect) {
      words.push({ text: one.text, isDynamic: one.isDynamic })
      continue
    }
    // A bare operator's target is the next word: `> out.json`.
    const operator = REDIRECT.exec(one.text)?.[0]
    if (operator === one.text) k++
  }

  return words
}

type Flags = { longs: ReadonlySet<string>; shorts: string }

const subtreeFlags = new Map<CatalogNode, Flags>()

// The value flags a word may belong to while the parse stands at `node`: the
// globals, plus every flag of the subtree below a group (clap re-reads a flag
// in front of an implied verb as the verb's), or exactly the leaf's own.
function flagsAt(node: CatalogNode): Flags {
  const known = subtreeFlags.get(node)
  if (known !== undefined) return known

  const longs = new Set([...GLOBAL_VALUES, ...node.values])
  let shorts = GLOBAL_SHORTS + node.shorts
  for (const child of Object.values(node.sub)) {
    const below = flagsAt(child)
    for (const long of below.longs) longs.add(long)
    shorts += below.shorts
  }
  const flags = { longs, shorts }
  subtreeFlags.set(node, flags)

  return flags
}

type Arg =
  | { kind: 'positional'; word: Word; at: number }
  // `name` keeps its dashes; `width` is how many words the option spans.
  | { kind: 'option'; name: string; value: Word | null; at: number; width: number }

// Sorts argv into the subcommand path, positionals and options the way clap
// reads it: `--flag value`, `--flag=value`, `-q value`, `-qvalue`, `-q=value`,
// a short cluster ending in a value flag (`-dq value`), and `--` ending the
// options. A word names a subcommand only until the first positional.
function scan(w: readonly Word[], from: number) {
  const args: Arg[] = []
  const path: { name: string; at: number }[] = []
  let node = CATALOG
  let isPastOptions = false

  for (let i = from; i < w.length; i++) {
    const word = w[i]
    if (word === undefined) break
    const { text } = word
    const hasPositional = args.some(arg => arg.kind === 'positional')
    const child = !hasPositional && Object.hasOwn(node.sub, text) ? node.sub[text] : undefined
    if (isPastOptions || !text.startsWith('-') || text === '-') {
      if (child !== undefined && !isPastOptions) {
        node = child
        path.push({ name: text, at: i })
      } else {
        args.push({ kind: 'positional', word, at: i })
      }
      continue
    }
    if (text === '--') {
      isPastOptions = true
      continue
    }
    const flags = flagsAt(node)
    if (text.startsWith('--')) {
      const eq = text.indexOf('=')
      const next = w[i + 1]
      if (eq > 0) {
        const value = { text: text.slice(eq + 1), isDynamic: word.isDynamic }
        args.push({ kind: 'option', name: text.slice(0, eq), value, at: i, width: 1 })
      } else if (flags.longs.has(text.slice(2)) && next !== undefined) {
        args.push({ kind: 'option', name: text, value: next, at: i, width: 2 })
        i++
      } else {
        args.push({ kind: 'option', name: text, value: null, at: i, width: 1 })
      }
      continue
    }
    for (let k = 1; k < text.length; k++) {
      const name = `-${text.charAt(k)}`
      if (!flags.shorts.includes(text.charAt(k))) {
        args.push({ kind: 'option', name, value: null, at: i, width: 1 })
        continue
      }
      const attached = text.slice(k + 1).replace(/^=/, '')
      const next = w[i + 1]
      if (attached !== '') {
        const value = { text: attached, isDynamic: word.isDynamic }
        args.push({ kind: 'option', name, value, at: i, width: 1 })
      } else if (next !== undefined) {
        args.push({ kind: 'option', name, value: next, at: i, width: 2 })
        i++
      } else {
        args.push({ kind: 'option', name, value: null, at: i, width: 1 })
      }
      break
    }
  }

  return { args, path, node }
}

function quoteForDisplay(word: string): string {
  return /[\s^|&;]/.test(word) ? `"${word}"` : word
}

// Which part of a dynamic `table:$ID` word the shell fills in.
function isExpanded(word: Word | undefined, part: string): boolean {
  return word?.isDynamic === true && /[$`]/.test(part)
}

// The first `sn …` in a shell command, or null when it has none.
export function parseSn(command: string): SnInvocation | null {
  let isInBackticks = false
  let before: Segment | null = null

  for (const seg of segments(command)) {
    const previous = before
    before = seg
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

    const scanned = scan(w, i + 1)
    let { node } = scanned
    const path = scanned.path.map(step => step.name)
    const positionals = scanned.args.filter(arg => arg.kind === 'positional')
    const options = scanned.args.filter(arg => arg.kind === 'option')

    // `sn table incident` lists; a reference or a second positional gets.
    const group = path[path.length - 1]
    if (path.length === 1 && group !== undefined && IMPLIED_VERB_GROUPS.has(group)) {
      const first = positionals[0]?.word.text ?? ''
      const verb = first.includes(':') || positionals.length > 1 ? 'get' : 'list'
      const leaf = node.sub[verb]
      if (leaf !== undefined) {
        node = leaf
        path.push(verb)
      }
    }

    const operands: Record<string, string> = {}
    const named = new Map<string, Arg & { kind: 'positional' }>()
    node.operands.forEach((name, k) => {
      const arg = positionals[k]
      if (arg === undefined) return
      operands[name] = arg.word.text
      named.set(name, arg)
    })

    // The table: a table-like positional, or the front of a `table:ref` reference.
    const tableArg = node.operands.map(name => named.get(name)).find((arg, k) => {
      const name = node.operands[k] ?? ''
      return arg !== undefined && (TABLE_OPERANDS.has(name) || name === 'reference')
    })
    const tableText = tableArg?.word.text ?? null
    const colon = tableText?.indexOf(':') ?? -1
    const table = tableText === null ? null : colon > 0 ? tableText.slice(0, colon) : tableText
    let record: string | null = null
    let recordWord: Word | undefined
    if (tableText !== null && colon > 0) {
      record = tableText.slice(colon + 1)
      recordWord = tableArg?.word
    } else {
      const recordName = [...node.operands].reverse().find(name => RECORD_OPERANDS.has(name) && named.has(name))
      const recordArg = recordName === undefined ? undefined : named.get(recordName)
      record = recordArg?.word.text ?? null
      recordWord = recordArg?.word
    }
    if (record !== null && isExpanded(recordWord, record)) record = null

    const query = options.find(opt => QUERY_FLAGS.has(opt.name))?.value ?? null
    const profile = options.find(opt => PROFILE_FLAGS.has(opt.name))?.value ?? null

    const hidden = new Set<number>(scanned.path.map(step => step.at))
    if (tableArg !== undefined) hidden.add(tableArg.at)
    // A script is shown as code, not as an argument.
    const scriptArg = named.get('script')
    if (scriptArg !== undefined) hidden.add(scriptArg.at)
    for (const opt of options) {
      if (PROFILE_FLAGS.has(opt.name)) for (let k = 0; k < opt.width; k++) hidden.add(opt.at + k)
    }
    const rest = w.filter((_, at) => at > i && !hidden.has(at)).map(word => word.text)
    if (tableText !== null && colon > 0) rest.unshift(tableText.slice(colon + 1))

    return {
      bin: bin.startsWith('/') ? bin : 'sn',
      path,
      command: path.length === 0 ? 'sn' : path.join(' '),
      table,
      record,
      query: query?.text ?? null,
      operands,
      args: rest.map(quoteForDisplay).join(' '),
      profile: profile?.text ?? null,
      isPiped: seg.sep === '|',
      pipeAt: seg.sep === '|' && command.charAt(seg.end + 1) !== '&' ? seg.end : null,
      stdinPipeAt: previous?.sep === '|' ? previous.end : null,
      heredoc: heredocOf(command, seg.end),
      isDynamic:
        isCutShort ||
        (table !== null && isExpanded(tableArg?.word, table)) ||
        query?.isDynamic === true ||
        profile?.isDynamic === true,
    }
  }

  return null
}

// The body of a heredoc opened on the line that ends at `end`: `<<EOF`,
// `<<'EOF'`, `<<"EOF"` or `<<-EOF`, closed by its word alone on a line.
function heredocOf(command: string, end: number): string | null {
  const lineStart = command.lastIndexOf('\n', end - 1) + 1
  const opener = /<<-?\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)\1/.exec(command.slice(lineStart, end))
  if (opener === null || command.charAt(end) !== '\n') return null
  const word = opener[2] ?? ''
  const lines = command.slice(end + 1).split('\n')
  const close = lines.findIndex(line => line.replace(/^\t+/, '').trimEnd() === word)

  return close < 0 ? null : lines.slice(0, close).join('\n')
}

function shellQuote(text: string): string {
  return `'${text.replace(/'/g, `'\\''`)}'`
}

// The command with sn's stdout also copied to `path`, by a `tee` between sn
// and the pipe: the pipeline's output and exit status stay what they were.
export function teeCommand(command: string, pipeAt: number, path: string): string {
  return `${command.slice(0, pipeAt)}| tee ${shellQuote(path)} ${command.slice(pipeAt)}`
}
