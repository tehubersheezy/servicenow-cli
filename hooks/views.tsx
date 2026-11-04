// How an entry draws. Each command can have a component of its own in
// ENTRY_VIEWS; every other one is a RunEntry. A component gets the surface's
// element table and the entry, and calls back for what its buttons do: the
// state it reads (expanded or not) lives in the pane's atoms, not in the
// component, because a drawing here is plain data with no state of its own.

import type { ElementTable, RenderNode } from 'claude-code'

import type { SnPanelRun } from '../types'
import { LOOK } from './kinds'
import { formatMs } from './outcome'

export type EntryProps = {
  ui: ElementTable
  run: SnPanelRun
  isExpanded: boolean
  onToggle: () => void
  onOpen: () => void
  onOutput: () => void
}

const STATUS_GLYPH = { running: '…', ok: '✓', failed: '✗' } as const
const STATUS_COLOR = { running: 'yellow', ok: 'green', failed: 'red' } as const
// An expanded section shows at most this many lines of code or output.
const SECTION_LINES = 200

function EntryHeader({ ui, run, onOpen, onOutput }: EntryProps) {
  const { Box, Button, Text } = ui
  const look = LOOK[run.kind]
  const isGone = run.kind === 'delete' && run.status === 'ok'
  const subject = [run.table ?? '', run.profile ? `@${run.profile}` : ''].filter(part => part !== '').join(' ')

  return (
    <Box flexDirection="row" justifyContent="space-between">
      <Box flexDirection="row" gap={1} flexShrink={1}>
        <Text color={STATUS_COLOR[run.status]} bold>
          {STATUS_GLYPH[run.status]}
        </Text>
        <Text color={look.color} bold wrap="truncate-end">
          {`${look.glyph} ${run.command}`}
        </Text>
        {subject !== '' && (
          <Text strikethrough={isGone} wrap="truncate-end">
            {subject}
          </Text>
        )}
      </Box>
      <Box flexDirection="row" gap={1}>
        <Text dimColor>{run.ms === null ? '' : formatMs(run.ms)}</Text>
        {typeof run.saved === 'string' && <Button key={`output-${run.id}`} label="Output" onPress={onOutput} />}
        {Array.isArray(run.open) && <Button key={`open-${run.id}`} label="Open" onPress={onOpen} />}
      </Box>
    </Box>
  )
}

function Args({ ui, run }: EntryProps) {
  const { Text } = ui
  if (run.args === '') return null

  return (
    <Text dimColor wrap="truncate-end">
      {`  ${run.args}`}
    </Text>
  )
}

function Summary({ ui, run }: EntryProps) {
  const { Text } = ui
  const color = run.status === 'failed' ? 'red' : run.status === 'running' ? 'yellow' : undefined

  return (
    <Text color={color} wrap="truncate-end">
      {`  ${run.summary}`}
    </Text>
  )
}

// A titled, framed block of lines: code with its line numbers, or output.
function Section({
  ui,
  title,
  lines,
  color,
  isCode,
  id,
}: {
  ui: ElementTable
  title: string
  lines: readonly string[]
  color: string
  isCode: boolean
  id: string
}) {
  const { Box, Text } = ui
  const shown = lines.slice(0, SECTION_LINES)
  const width = String(shown.length).length

  return (
    <Box flexDirection="column" marginLeft={2} marginTop={1}>
      <Text dimColor>{title}</Text>
      <Box flexDirection="column" borderStyle="round" borderColor={color} paddingX={1}>
        {shown.length === 0 && <Text dimColor>(nothing)</Text>}
        {shown.map((line, k) => (
          <Box key={`${id}-${k}`} flexDirection="row" gap={1}>
            {isCode && <Text dimColor>{String(k + 1).padStart(width)}</Text>}
            <Text dimColor={!isCode && line.startsWith('· ')} wrap="truncate-end">
              {line.replace(/\t/g, '  ') || ' '}
            </Text>
          </Box>
        ))}
        {lines.length > shown.length && <Text dimColor>{`… ${lines.length - shown.length} more`}</Text>}
      </Box>
    </Box>
  )
}

// Any command: header, its arguments, what it came to, and lines it printed.
export function RunEntry(props: EntryProps) {
  const { ui, run } = props
  const { Box } = ui

  return (
    <Box flexDirection="column" marginTop={1}>
      <EntryHeader {...props} />
      <Args {...props} />
      <Summary {...props} />
      {run.lines.length > 0 && (
        <Section ui={ui} title="output" lines={run.lines} color={LOOK[run.kind].color} isCode={false} id={run.id} />
      )}
    </Box>
  )
}

// `sn script run`: the script and what it logged, folded. The entry has no
// buttons of its own: its header and summary rows are the toggle. (Only a
// Button takes a press, and a Button is one line of text, so those two rows
// are the click target.)
export function ScriptRunEntry(props: EntryProps) {
  const { ui, run, isExpanded, onToggle } = props
  const { Box, Button, Text } = ui
  const look = LOOK[run.kind]
  const script = typeof run.script === 'string' ? run.script.replace(/\n+$/, '').split('\n') : null
  const from = run.scriptFrom === 'inline' || run.scriptFrom == null ? 'script' : `script · ${run.scriptFrom}`
  const chevron = isExpanded ? '▾' : '▸'
  const title = [`${chevron} ${run.command}`, run.profile ? `@${run.profile}` : ''].filter(part => part !== '').join(' ')

  return (
    <Box flexDirection="column" marginTop={1}>
      <Box flexDirection="row" justifyContent="space-between">
        <Box flexDirection="row" gap={1} flexShrink={1}>
          <Text color={STATUS_COLOR[run.status]} bold>
            {STATUS_GLYPH[run.status]}
          </Text>
          <Text color={look.color} bold>
            {look.glyph}
          </Text>
          <Button key={`toggle-${run.id}`} label={title} plain onPress={onToggle} />
        </Box>
        <Text dimColor>{run.ms === null ? '' : formatMs(run.ms)}</Text>
      </Box>
      <Args {...props} />
      <Box paddingLeft={2}>
        <Button key={`toggle-summary-${run.id}`} label={run.summary} plain dimColor={run.status === 'running'} onPress={onToggle} />
      </Box>
      {isExpanded && (
        <Box flexDirection="column">
          {script === null ? (
            <Box marginLeft={2} marginTop={1}>
              <Text dimColor>{`${from}: not readable from here`}</Text>
            </Box>
          ) : (
            <Section ui={ui} title={from} lines={script} color="gray" isCode id={`${run.id}-script`} />
          )}
          <Section
            ui={ui}
            title="output"
            lines={run.lines}
            color={run.status === 'failed' ? 'red' : look.color}
            isCode={false}
            id={`${run.id}-out`}
          />
        </Box>
      )}
    </Box>
  )
}

// The component each command draws with; the rest draw as RunEntry.
export const ENTRY_VIEWS: Record<string, (props: EntryProps) => RenderNode | null> = {
  'script run': ScriptRunEntry,
}

export function Entry(props: EntryProps) {
  const View = ENTRY_VIEWS[props.run.command] ?? RunEntry

  return <View {...props} />
}
