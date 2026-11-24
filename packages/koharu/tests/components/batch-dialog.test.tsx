import { QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { BatchDialog } from '@/components/app/BatchDialog'
import { queryClient } from '@/lib/queries'
import { type Preferences, commands } from '@koharu/ipc/protocol'
import { toast } from '@koharu/ui/components/toast'

/** The dialog, with `target` as the translator's target language. */
function renderDialog(target = 'en') {
  vi.spyOn(commands, 'getPreferences').mockResolvedValue({
    pipeline: { translator: { target } },
  } as unknown as Preferences)
  render(
    <QueryClientProvider client={queryClient}>
      <BatchDialog open onOpenChange={() => undefined} />
    </QueryClientProvider>,
  )
}

describe('BatchDialog', () => {
  it('shows why the folder picker did not open', async () => {
    const reason = 'the folder dialog could not be opened'
    vi.spyOn(commands, 'pickBatchFolder').mockRejectedValue(reason)
    const add = vi.spyOn(toast, 'add').mockImplementation(() => 'toast')

    renderDialog()
    fireEvent.click(screen.getAllByRole('button', { name: 'Choose…' })[0])

    await waitFor(() =>
      expect(add).toHaveBeenCalledWith(expect.objectContaining({ type: 'error', description: reason })),
    )
  })

  it('shows why the batch did not start, and translates into the saved target', async () => {
    const reason = 'koharu-batch.exe was not found'
    vi.spyOn(commands, 'pickBatchFolder')
      .mockResolvedValueOnce('C:/pages')
      .mockResolvedValueOnce('C:/output')
    const start = vi.spyOn(commands, 'startBatch').mockRejectedValue(reason)
    const add = vi.spyOn(toast, 'add').mockImplementation(() => 'toast')

    renderDialog('ja')

    const [source, output] = screen.getAllByRole('button', { name: 'Choose…' })
    fireEvent.click(source)
    fireEvent.click(output)
    const startButton = screen.getByRole('button', { name: 'Start' })
    await waitFor(() => expect(startButton).toBeEnabled())

    fireEvent.click(startButton)

    await waitFor(() =>
      expect(add).toHaveBeenCalledWith(expect.objectContaining({ type: 'error', description: reason })),
    )
    expect(start.mock.calls[0]?.[2]).toBe('ja')
  })
})
