import { http, HttpResponse } from 'msw'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { queueAutoRender } from '@/lib/io/scene'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

import { server } from '../../msw/server'

describe('queueAutoRender', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('coalesces rapid edits into a single pipeline POST and forwards default font', async () => {
    vi.spyOn(usePreferencesStore, 'getState').mockReturnValue({
      defaultFont: 'Comic Sans MS',
    } as ReturnType<typeof usePreferencesStore.getState>)
    const pipelineHits: Array<{ steps: string[]; pages: string[]; defaultFont?: string | null }> =
      []
    server.use(
      http.get('/api/v1/config', () =>
        HttpResponse.json({ pipeline: { renderer: 'koharu-renderer' } }),
      ),
      http.post('/api/v1/pipelines', async ({ request }) => {
        const body = (await request.json()) as {
          steps: string[]
          pages: string[]
          defaultFont?: string | null
        }
        pipelineHits.push({
          steps: body.steps,
          pages: body.pages,
          defaultFont: body.defaultFont,
        })
        return HttpResponse.json({ operationId: `op-${pipelineHits.length}` })
      }),
    )

    queueAutoRender('p-1')
    queueAutoRender('p-1')
    queueAutoRender('p-1')

    // Before the debounce window elapses, no POST.
    expect(pipelineHits).toHaveLength(0)

    // Debounce = 500ms. Advance just past it and let any pending microtasks run.
    await vi.advanceTimersByTimeAsync(550)

    expect(pipelineHits).toHaveLength(1)
    expect(pipelineHits[0].steps).toEqual(['koharu-renderer'])
    expect(pipelineHits[0].pages).toEqual(['p-1'])
    expect(pipelineHits[0].defaultFont).toBe('Comic Sans MS')
  })

  it('is a no-op when no renderer is configured', async () => {
    vi.spyOn(usePreferencesStore, 'getState').mockReturnValue({
      defaultFont: undefined,
    } as ReturnType<typeof usePreferencesStore.getState>)
    let pipelinePosts = 0
    server.use(
      http.get('/api/v1/config', () => HttpResponse.json({ pipeline: {} })),
      http.post('/api/v1/pipelines', () => {
        pipelinePosts += 1
        return HttpResponse.json({ operationId: 'op-1' })
      }),
    )

    queueAutoRender('p-1')
    await vi.advanceTimersByTimeAsync(550)

    expect(pipelinePosts).toBe(0)
  })
})
