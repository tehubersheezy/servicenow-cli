export type SnPanelStatus = 'running' | 'ok' | 'failed'

export type SnPanelCall = {
  id: string
  bin: string
  verb: string
  table: string
  record: string | null
  query: string | null
  args: string
  profile: string | null
  isPiped: boolean
  isDynamic: boolean
  status: SnPanelStatus
  summary: string
  records: number | null
  ms: number | null
  // The `sn open` argv behind the entry's Open button; null shows no button.
  open: string[] | null
}

export type SnPanelTotals = {
  calls: number
  ok: number
  failed: number
  records: number
  byTable: Record<string, number>
}

declare module 'claude-code' {
  interface PluginState {
    sn: {
      calls: SnPanelCall[]
      totals: SnPanelTotals
      autoOpened: boolean
    }
  }
}
