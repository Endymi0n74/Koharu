import { QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { BatchDialog } from '@/components/app/BatchDialog'
import { queryClient } from '@/lib/queries'
import { type Preferences, commands } from '@koharu/ipc/protocol'
import { toast } from '@koharu/ui/components/toast'

// A channel takes its id from the desktop as it is made.
beforeEach(() => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', {
    value: { transformCallback: () => 1 },
    writable: true,
    configurable: true,
  })
})

/** The dialog, with `target` as the translator's target language. */
function renderDialog(target = 'en') {
  vi.spyOn(commands, 'getPreferences').mockResolvedValue({
    pipeline: { translator: { target } },
  } as unknown as Preferences)
  vi.spyOn(commands, 'listModels').mockResolvedValue([])
  return render(
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
      expect(add).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'error', description: reason }),
      ),
    )
  })

  it('shows why the batch did not start, and translates into the saved target', async () => {
    const reason = 'koharu-batch.exe was not found'
    vi.spyOn(commands, 'pickBatchFolder').mockResolvedValue('C:/pages')
    vi.spyOn(commands, 'pickBatchOutput').mockResolvedValue('C:/output')
    const start = vi.spyOn(commands, 'startBatch').mockRejectedValue(reason)
    const add = vi.spyOn(toast, 'add').mockImplementation(() => 'toast')

    renderDialog('fr-FR')
    expect(screen.getByRole('combobox', { name: 'Target language' })).toHaveTextContent('French')

    const [source, output] = screen.getAllByRole('button', { name: 'Choose…' })
    fireEvent.click(source)
    fireEvent.click(output)
    const startButton = screen.getByRole('button', { name: 'Start' })
    await waitFor(() => expect(startButton).toBeEnabled())

    fireEvent.click(startButton)

    await waitFor(() =>
      expect(add).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'error', description: reason }),
      ),
    )
    expect(start.mock.calls[0]?.[2]).toBe('fr-FR')
  })

  it('refuses the input folder as the output folder', async () => {
    vi.spyOn(commands, 'pickBatchFolder').mockResolvedValue('C:/pages')
    vi.spyOn(commands, 'pickBatchOutput').mockResolvedValue('C:/pages')
    const add = vi.spyOn(toast, 'add').mockImplementation(() => 'toast')

    renderDialog()
    const [source, output] = screen.getAllByRole('button', { name: 'Choose…' })
    fireEvent.click(source)
    await waitFor(() => expect(screen.getAllByRole('textbox')[0]).toHaveValue('C:/pages'))
    fireEvent.click(output)

    await waitFor(() =>
      expect(add).toHaveBeenCalledWith(expect.objectContaining({ type: 'error' })),
    )
    expect(screen.getAllByRole('textbox')[1]).toHaveValue('')
    expect(screen.getByRole('button', { name: 'Start' })).toBeDisabled()
  })

  it('counts the run’s page stages instead of showing its lines', async () => {
    vi.spyOn(commands, 'pickBatchFolder').mockResolvedValue('C:/pages')
    vi.spyOn(commands, 'pickBatchOutput').mockResolvedValue('C:/output')
    let emit: (line: string) => void = () => undefined
    const start = vi
      .spyOn(commands, 'startBatch')
      .mockImplementation((_input, _output, _lang, _translator, channel) => {
        emit = (line) => channel.onmessage?.(line)
        return new Promise<string>(() => undefined)
      })

    renderDialog()

    const [source, output] = screen.getAllByRole('button', { name: 'Choose…' })
    fireEvent.click(source)
    fireEvent.click(output)
    const startButton = screen.getByRole('button', { name: 'Start' })
    await waitFor(() => expect(startButton).toBeEnabled())
    fireEvent.click(startButton)
    await waitFor(() => expect(start).toHaveBeenCalled())

    act(() => emit('8 page stages to run'))
    expect(screen.getByText('0/8 steps')).toBeInTheDocument()
    act(() => {
      for (const stage of ['detect', 'recognize', 'inpaint']) {
        emit(`${stage} done on 01a205f0-0000-7000-8000-000000000000`)
      }
    })
    expect(screen.getByText('3/8 steps')).toBeInTheDocument()
  })
})
