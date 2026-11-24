'use client'

import { useCallback, useMemo, useState } from 'react'

import { commands } from '@koharu/ipc/protocol'
import { TARGET_LANGUAGES } from '@/lib/translation'
import { Button } from '@koharu/ui/components/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@koharu/ui/components/dialog'
import { Input } from '@koharu/ui/components/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'
import { Switch } from '@koharu/ui/components/switch'

/** Lets the user pick a source folder, an output project, and a target language,
 * then runs `koharu-batch` over the folder. */
export function BatchDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [source, setSource] = useState<string | null>(null)
  const [output, setOutput] = useState<string | null>(null)
  const [lang, setLang] = useState<string>('fr-FR')
  const [busy, setBusy] = useState(false)

  const languageChoices = useMemo(
    () => TARGET_LANGUAGES.map((tag) => ({ tag, name: tag })),
    [],
  )
  const ready = Boolean(source && output) && !busy

  const pick = (setPath: (path: string) => void) => {
    void commands.pickBatchFolder().then((path) => {
      if (path) setPath(path)
    })
  }

  const start = async () => {
    if (!source || !output) return
    setBusy(true)
    try {
      await commands.startBatch(source, output, lang)
      onOpenChange(false)
    } catch {
      // error is surfaced by Tauri as a toast via send in queries.ts
    } finally {
      setBusy(false)
    }
  }

  const folderRow = (
    label: string,
    path: string | null,
    setPath: (path: string) => void,
  ) => (
    <label className='grid gap-1.5'>
      <span className='text-[11px] font-medium text-muted-foreground'>{label}</span>
      <span className='flex gap-2'>
        <Input
          readOnly
          value={path ?? ''}
          placeholder='None selected'
          className='h-8 flex-1 text-[11px]'
        />
        <Button
          type='button'
          variant='outline'
          size='sm'
          className='h-8 shrink-0 text-[11px]'
          onClick={() => pick(setPath)}
        >
          Choose…
        </Button>
      </span>
    </label>
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='max-w-md gap-4 p-5'>
        <DialogHeader className='gap-1'>
          <DialogTitle className='text-[15px]'>Batch translate a folder</DialogTitle>
          <DialogDescription className='text-[11px]'>
            Translate every page of a folder with koharu-batch, outside any project.
          </DialogDescription>
        </DialogHeader>

        <div className='grid gap-3'>
          {folderRow('Folder to translate', source, setSource)}
          {folderRow('Output project', output, setOutput)}

          <label className='grid gap-1.5'>
            <span className='text-[11px] font-medium text-muted-foreground'>
              Target language
            </span>
            <Select value={lang} onValueChange={(tag) => tag && setLang(tag)}>
              <SelectTrigger
                aria-label='Target language'
                className='h-8 w-full text-[11px]'
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {languageChoices.map((choice) => (
                  <SelectItem key={choice.tag} value={choice.tag}>
                    {choice.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
        </div>

        <DialogFooter>
          <Button
            type='button'
            size='sm'
            className='h-8 text-[11px]'
            disabled={!ready}
            aria-busy={busy}
            onClick={start}
          >
            {busy ? 'Starting…' : 'Start'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
