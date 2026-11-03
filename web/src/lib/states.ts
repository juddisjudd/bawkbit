import ArrowDown from '@lucide/svelte/icons/arrow-down'
import ArrowUp from '@lucide/svelte/icons/arrow-up'
import Check from '@lucide/svelte/icons/check'
import CircleAlert from '@lucide/svelte/icons/circle-alert'
import Clock from '@lucide/svelte/icons/clock'
import LoaderCircle from '@lucide/svelte/icons/loader-circle'
import Magnet from '@lucide/svelte/icons/magnet'
import Pause from '@lucide/svelte/icons/pause'
import RefreshCw from '@lucide/svelte/icons/refresh-cw'
import type { Component } from 'svelte'
import type { State, Torrent } from './api'

interface Look {
  icon: Component<{ size?: number; class?: string }>
  color: string
  label: string
  spin?: boolean
}

export const looks: Record<State, Look> = {
  metadata: { icon: Magnet, color: 'var(--folder-purple)', label: 'fetching metadata' },
  checking: { icon: RefreshCw, color: 'var(--folder-blue)', label: 'checking files', spin: true },
  downloading: { icon: ArrowDown, color: 'var(--yolk)', label: 'downloading' },
  stalled: { icon: ArrowDown, color: 'var(--warning)', label: 'stalled, no peers sending' },
  seeding: { icon: ArrowUp, color: 'var(--success)', label: 'seeding' },
  done: { icon: Check, color: 'var(--text-weak)', label: 'done' },
  paused: { icon: Pause, color: 'var(--text-weaker)', label: 'paused' },
  queued: { icon: Clock, color: 'var(--text-weaker)', label: 'queued' },
  error: { icon: CircleAlert, color: 'var(--danger)', label: 'error' }
}

export const loading = LoaderCircle

export function isRunning(t: Torrent): boolean {
  return t.state !== 'paused' && t.state !== 'done' && t.state !== 'error'
}

export function barColor(t: Torrent): string {
  if (t.state === 'done') return 'var(--success)'
  return looks[t.state].color
}
