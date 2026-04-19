import { beforeEach, describe, expect, it } from 'vitest'

import { getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import type { Node, Page, SceneSnapshot } from '@/lib/api/schemas'
import { selectAllTextNodesOnCurrentPage } from '@/lib/io/scene'
import { queryClient } from '@/lib/queryClient'
import { useSelectionStore } from '@/lib/stores/selectionStore'

function textNode(id: string): Node {
  return {
    id,
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: { text: { raw: `t-${id}` } },
  } as unknown as Node
}

function imageNode(id: string): Node {
  return {
    id,
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: {
      image: { role: 'source', blob: `b-${id}`, opacity: 1, naturalWidth: 1, naturalHeight: 1 },
    },
  } as unknown as Node
}

function seedScene(): SceneSnapshot {
  const page: Page = {
    id: 'p-1',
    name: 'P',
    width: 10,
    height: 10,
    nodes: {
      src: imageNode('src'),
      t1: textNode('t1'),
      t2: textNode('t2'),
      rend: imageNode('rend'),
    },
  } as unknown as Page
  return {
    epoch: 1,
    scene: { pages: { 'p-1': page }, project: { name: 'P' } as never } as never,
  }
}

describe('selectAllTextNodesOnCurrentPage', () => {
  beforeEach(() => {
    useSelectionStore.getState().setPage(null)
    queryClient.clear()
  })

  it('is a no-op when no page is selected', () => {
    queryClient.setQueryData(getGetSceneJsonQueryKey(), seedScene())
    selectAllTextNodesOnCurrentPage()
    expect(useSelectionStore.getState().nodeIds.size).toBe(0)
  })

  it('is a no-op when the scene snapshot is not cached', () => {
    useSelectionStore.getState().setPage('p-1')
    selectAllTextNodesOnCurrentPage()
    expect(useSelectionStore.getState().nodeIds.size).toBe(0)
  })

  it('selects only text nodes on the active page', () => {
    queryClient.setQueryData(getGetSceneJsonQueryKey(), seedScene())
    useSelectionStore.getState().setPage('p-1')
    selectAllTextNodesOnCurrentPage()
    expect([...useSelectionStore.getState().nodeIds].sort()).toEqual(['t1', 't2'])
  })

  it('replaces existing selection with the full text-node set', () => {
    queryClient.setQueryData(getGetSceneJsonQueryKey(), seedScene())
    useSelectionStore.getState().setPage('p-1')
    useSelectionStore.getState().select('src', false)
    expect(useSelectionStore.getState().nodeIds.has('src')).toBe(true)
    selectAllTextNodesOnCurrentPage()
    expect([...useSelectionStore.getState().nodeIds].sort()).toEqual(['t1', 't2'])
  })
})
