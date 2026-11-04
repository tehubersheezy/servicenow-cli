export type SnPanelStatus = 'running' | 'ok' | 'failed'

export type SnPanelKind = 'read' | 'write' | 'delete' | 'run' | 'stream' | 'session' | 'skill'

export type SnPanelRun = {
  id: string
  bin: string
  // The subcommand path joined with spaces: `table list`, `script run`.
  command: string
  kind: SnPanelKind
  table: string | null
  record: string | null
  query: string | null
  args: string
  profile: string | null
  isPiped: boolean
  isDynamic: boolean
  status: SnPanelStatus
  summary: string
  // Shown under the summary, as printed: what a script logged.
  lines: string[]
  records: number | null
  ms: number | null
  // The `sn open` argv behind the entry's Open button; null shows no button.
  open: string[] | null
  // The file holding what the call printed, behind the Output button. Kept out
  // of the model's context: a piped call's copy comes from a tee.
  saved: string | null
  // A `script run`'s script, as far as the hook could know it, and where it
  // came from: `inline`, `heredoc`, `stdin` or the file's path.
  script?: string | null
  scriptFrom?: string | null
}

export type SnPanelStats = {
  calls: number
  ok: number
  failed: number
  records: number
  byKind: Partial<Record<SnPanelKind, number>>
  byCommand: Record<string, number>
}

declare module 'claude-code' {
  interface PluginState {
    sn: {
      entries: SnPanelRun[]
      stats: SnPanelStats
      // The one kind the pane lists, or null for all of them.
      filter: SnPanelKind | null
      // The entries drawn expanded.
      expanded: string[]
    }
  }
}
