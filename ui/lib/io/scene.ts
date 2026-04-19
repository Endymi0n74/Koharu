'use client'

import {
  applyCommand,
  createPages,
  createPagesFromPaths,
  createProject,
  deleteCurrentProject,
  exportCurrentProject,
  getConfig,
  getGetConfigQueryKey,
  getGetCurrentLlmQueryKey,
  getGetSceneJsonQueryKey,
  importProject,
  patchConfig,
  putCurrentProject,
  redo,
  startPipeline,
  undo,
} from '@/lib/api/default/default'
import type {
  ConfigPatch,
  CreateProjectRequest,
  ExportProjectRequest,
  Op,
  OpenProjectRequest,
  ProjectSummary,
  SceneSnapshot,
} from '@/lib/api/schemas'
import { queryClient } from '@/lib/queryClient'
import { useSelectionStore } from '@/lib/stores/selectionStore'

/**
 * Imperative action helpers. Every mutation below is a thin wrapper that
 *   1. calls the orval-generated request function (never raw `fetch`), and
 *   2. invalidates the React Query cache entries affected by the change.
 *
 * The UI reads scene / config / llm state via the generated `useGet*` hooks;
 * after each mutation React Query refetches — no client-side scene reducer,
 * no optimistic mirroring, backend is the single source of truth.
 */

const invalidateScene = () => queryClient.invalidateQueries({ queryKey: getGetSceneJsonQueryKey() })

const invalidateConfig = () => queryClient.invalidateQueries({ queryKey: getGetConfigQueryKey() })

const invalidateLlm = () => queryClient.invalidateQueries({ queryKey: getGetCurrentLlmQueryKey() })

// Ops ------------------------------------------------------------------------

export async function applyOp(op: Op): Promise<void> {
  await applyCommand(op)
  await invalidateScene()
}

export async function undoOp(): Promise<void> {
  await undo()
  await invalidateScene()
}

export async function redoOp(): Promise<void> {
  await redo()
  await invalidateScene()
}

// Auto-render ---------------------------------------------------------------
//
// `queueAutoRender(pageId)` schedules a debounced renderer-pipeline invocation
// so a text-block edit (move/resize/translation/color/etc.) produces an
// updated rendered image without the user running Render manually.
//
// Coalescing is essential: slider drags and typing emit many ops per second;
// the trailing-edge debounce fires one render after the edits settle.

const AUTO_RENDER_DEBOUNCE_MS = 500

let autoRenderTimer: ReturnType<typeof setTimeout> | null = null
let autoRenderPendingPageId: string | null = null

export function queueAutoRender(pageId: string): void {
  autoRenderPendingPageId = pageId
  if (autoRenderTimer) clearTimeout(autoRenderTimer)
  autoRenderTimer = setTimeout(() => {
    autoRenderTimer = null
    const id = autoRenderPendingPageId
    autoRenderPendingPageId = null
    if (!id) return
    void runAutoRender(id)
  }, AUTO_RENDER_DEBOUNCE_MS)
}

async function runAutoRender(pageId: string): Promise<void> {
  try {
    const cfg = await getConfig()
    const renderer = cfg.pipeline?.renderer
    if (!renderer) return
    await startPipeline({ steps: [renderer], pages: [pageId] })
  } catch (err) {
    // Auto-render failures shouldn't disturb the editing flow; users can
    // always run Render manually from the toolbar / menu.
    console.error('Auto-render failed:', err)
  }
}

/** Select every text node on the active page. No-op if no project/page open. */
export function selectAllTextNodesOnCurrentPage(): void {
  const pageId = useSelectionStore.getState().pageId
  if (!pageId) return
  const snap = queryClient.getQueryData<SceneSnapshot>(getGetSceneJsonQueryKey())
  const page = snap?.scene?.pages?.[pageId]
  if (!page) return
  const ids: string[] = []
  for (const [id, node] of Object.entries(page.nodes)) {
    if (node && 'text' in node.kind) ids.push(id)
  }
  useSelectionStore.getState().selectMany(ids)
}

// Project lifecycle ----------------------------------------------------------

export async function createAndOpenProject(req: CreateProjectRequest): Promise<ProjectSummary> {
  const summary = await createProject(req)
  await invalidateScene()
  return summary
}

export async function switchProject(req: OpenProjectRequest): Promise<void> {
  await putCurrentProject(req)
  await invalidateScene()
}

export async function closeProject(): Promise<void> {
  await deleteCurrentProject()
  await invalidateScene()
}

// Pages import ---------------------------------------------------------------

export async function uploadPages(files: File[], replace: boolean): Promise<string[]> {
  const form = new FormData()
  for (const file of files) form.append('file', file, file.name)
  form.append('replace', replace ? 'true' : 'false')
  const res = await createPages({ body: form })
  await invalidateScene()
  return res.pages
}

/**
 * Tauri fast-path: hand the backend a list of absolute file paths. Skips
 * the per-file `readFile` IPC round-trip, skips JS-side buffering, skips
 * multipart upload — the Rust side reads + decodes + hashes in parallel.
 */
export async function uploadPagesByPaths(paths: string[], replace: boolean): Promise<string[]> {
  const res = await createPagesFromPaths({ paths, replace })
  await invalidateScene()
  return res.pages
}

export async function uploadKhrArchive(file: File): Promise<ProjectSummary> {
  const bytes = await file.arrayBuffer()
  const summary = await importProject({
    body: bytes,
    headers: { 'Content-Type': 'application/zip' },
  })
  await invalidateScene()
  return summary
}

// Export ---------------------------------------------------------------------

export async function exportProject(req: ExportProjectRequest): Promise<Blob> {
  return exportCurrentProject(req)
}

// Config ---------------------------------------------------------------------

export async function updateConfig(patch: ConfigPatch): Promise<void> {
  await patchConfig(patch)
  await invalidateConfig()
}

// LLM ------------------------------------------------------------------------

export function invalidateCurrentLlm(): Promise<void> {
  return invalidateLlm()
}
