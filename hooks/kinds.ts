// What kind of thing a command does, which decides how its entry looks. A new
// command is classified by its leaf verb (`create` writes, `delete` deletes),
// so only a command whose verb says nothing about it needs a line here.

import type { SnInvocation } from './parse'

export type Kind = 'read' | 'write' | 'delete' | 'run' | 'stream' | 'session' | 'skill'

export const KINDS: readonly Kind[] = ['read', 'write', 'delete', 'run', 'stream', 'session', 'skill']

export const LOOK: Record<Kind, { glyph: string; color: string }> = {
  read: { glyph: '◇', color: 'cyan' },
  write: { glyph: '✎', color: 'yellow' },
  delete: { glyph: '✕', color: 'red' },
  run: { glyph: '▶', color: 'magenta' },
  stream: { glyph: '◉', color: 'blue' },
  session: { glyph: '⚙', color: 'gray' },
  skill: { glyph: '★', color: 'green' },
}

const BY_COMMAND: Record<string, Kind> = {
  'cache refresh': 'session',
  'catalog checkout': 'write',
  'catalog submit-order': 'write',
  'change approvals': 'write',
  'change risk': 'write',
  'cicd app install': 'write',
  'cicd app publish': 'write',
  'cicd app rollback': 'delete',
  'cicd atf run': 'run',
  'cicd updateset back-out': 'delete',
  'cicd updateset commit': 'write',
  'cicd updateset commit-multiple': 'write',
  'cicd updateset preview': 'run',
  'cicd updateset retrieve': 'write',
  'decision run': 'run',
  'identify create-update': 'write',
  'identify create-update-enhanced': 'write',
  'scores favorite': 'write',
  'scores unfavorite': 'write',
  'xml import': 'write',
  'script run': 'run',
  completion: 'session',
  impersonate: 'session',
  init: 'session',
  open: 'session',
  sn: 'session',
  watch: 'stream',
}

const BY_GROUP: Record<string, Kind> = {
  context: 'session',
  profile: 'session',
  set: 'session',
}

const BY_VERB: Record<string, Kind> = {
  add: 'write',
  'add-to-cart': 'write',
  bulk: 'write',
  'cart-empty': 'delete',
  'cart-remove': 'delete',
  'cart-update': 'write',
  create: 'write',
  delete: 'delete',
  launch: 'run',
  order: 'write',
  remove: 'delete',
  run: 'run',
  set: 'write',
  tail: 'stream',
  trigger: 'run',
  update: 'write',
  upload: 'write',
}

const RAW_METHOD: Record<string, Kind> = { GET: 'read', HEAD: 'read', DELETE: 'delete' }

export function kindOf(call: Pick<SnInvocation, 'path' | 'command' | 'operands'>): Kind {
  // `sn raw` and `sn graphql` are whatever the request they carry is.
  if (call.command === 'raw') return RAW_METHOD[(call.operands.method ?? '').toUpperCase()] ?? 'write'
  if (call.command === 'graphql') return /^\s*mutation\b/.test(call.operands.query ?? '') ? 'write' : 'read'

  const group = call.path[0] ?? ''
  const verb = call.path[call.path.length - 1] ?? ''

  return BY_COMMAND[call.command] ?? BY_GROUP[group] ?? BY_VERB[verb] ?? 'read'
}

const PAST: Record<string, string> = {
  add: 'added',
  'add-to-cart': 'added to cart',
  'back-out': 'backed out',
  bulk: 'loaded',
  commit: 'committed',
  create: 'created',
  delete: 'deleted',
  install: 'installed',
  order: 'ordered',
  publish: 'published',
  remove: 'removed',
  retrieve: 'retrieved',
  rollback: 'rolled back',
  set: 'set',
  update: 'updated',
  upload: 'uploaded',
}

// What a write or delete did, for its summary: `created`, `deleted`. Null for a
// verb that does not read as one.
export function pastTense(verb: string): string | null {
  return PAST[verb] ?? null
}
