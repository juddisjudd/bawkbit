import type { SearchResult } from './api'

export type SearchSortKey = 'title' | 'size' | 'seeds' | 'peers' | 'published' | 'source'

export interface Refine {
  words: string[]
  without: string[]
  min: number | null
  max: number | null
}

const SCALE: Record<string, number> = {
  b: 1,
  k: 1024,
  kb: 1024,
  kib: 1024,
  m: 1024 ** 2,
  mb: 1024 ** 2,
  mib: 1024 ** 2,
  g: 1024 ** 3,
  gb: 1024 ** 3,
  gib: 1024 ** 3,
  t: 1024 ** 4,
  tb: 1024 ** 4,
  tib: 1024 ** 4
}

/** Reads `ubuntu -beta >1GB <4 gb`: words to keep, `-words` to drop, and size limits; a bare number is megabytes. */
export function parseRefine(text: string): Refine {
  const refine: Refine = { words: [], without: [], min: null, max: null }
  const joined = text.toLowerCase().replace(/([<>]=?)\s*(\d+(?:\.\d+)?)\s*(kib|mib|gib|tib|kb|mb|gb|tb|[bkmgt])?\b/g, '$1$2$3')
  for (const token of joined.split(/\s+/).filter(Boolean)) {
    const size = /^([<>])=?(\d+(?:\.\d+)?)(kib|mib|gib|tib|kb|mb|gb|tb|[bkmgt])?$/.exec(token)
    if (size) {
      const bytes = Number(size[2]) * SCALE[size[3] ?? 'mb']
      if (size[1] === '>') refine.min = bytes
      else refine.max = bytes
    } else if (token.startsWith('-') && token.length > 1) {
      refine.without.push(token.slice(1))
    } else {
      refine.words.push(token)
    }
  }
  return refine
}

export function refines(result: SearchResult, refine: Refine): boolean {
  const title = result.title.toLowerCase()
  return (
    refine.words.every((word) => title.includes(word)) &&
    !refine.without.some((word) => title.includes(word)) &&
    (refine.min === null || result.size >= refine.min) &&
    (refine.max === null || result.size <= refine.max)
  )
}

export const COMPARE_RESULTS: Record<SearchSortKey, (a: SearchResult, b: SearchResult) => number> = {
  title: (a, b) => a.title.localeCompare(b.title, undefined, { numeric: true, sensitivity: 'base' }),
  size: (a, b) => a.size - b.size,
  seeds: (a, b) => (a.seeds ?? -1) - (b.seeds ?? -1),
  peers: (a, b) => (a.peers ?? -1) - (b.peers ?? -1),
  published: (a, b) => (a.published ?? 0) - (b.published ?? 0),
  source: (a, b) => a.source.localeCompare(b.source, undefined, { sensitivity: 'base' })
}

/** How healthy a swarm looks from its seed count. */
export function seedLevel(seeds: number | null): 'none' | 'low' | 'good' | 'unknown' {
  if (seeds === null) return 'unknown'
  if (seeds === 0) return 'none'
  return seeds < 5 ? 'low' : 'good'
}
