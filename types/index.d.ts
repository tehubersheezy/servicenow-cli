export type SnPanelStatus = 'running' | 'ok' | 'failed'

export type SnPanelCall = {
  id: string
  verb: string
  table: string
  args: string
  profile: string | null
  isPiped: boolean
  status: SnPanelStatus
  summary: string
  records: number | null
  ms: number | null
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
