'use client'

import { useMutation } from '@tanstack/react-query'
import { useCallback, useMemo, useState } from 'react'

import { usePreferences } from '@/lib/queries'
import { commands } from '@koharu/ipc/protocol'
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

/** Lets the user pick a source folder, an output project, and options, then
 * runs `koharu-batch` over the folder. */
export function BatchDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const target = usePreferences().data?.pipeline.translator?.target ?? 'en'
  const [source, setSource] = useState<string | null>(null)
  const [output, setOutput] = useState<string | null>(null)
  const [translator, setTranslator] = useState<string>('')
  const [deterministic, setDeterministic] = useState(true)
  const [overwrite, setOverwrite] = useState(false)

  // A failed start shows its reason through the mutation cache's toast.
  const start = useMutation({
    mutationFn: () =>
      commands.startBatch(
        source ?? '',
        output ?? '',
        target,
        translator || undefined,
        deterministic,
        overwrite,
      ),
    onSuccess: () => onOpenChange(false),
  })

  const ready = Boolean(source && output) && !start.isPending

  const pick = useMutation({
    mutationFn: (setPath: (path: string) => void) =>
      commands.pickBatchFolder().then((path) => {
        if (path) setPath(path)
      }),
  })

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
          onClick={() => pick.mutate(setPath)}
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
              Model
            </span>
            <Input
              value={translator}
              placeholder='Auto'
              className='h-8 flex-1 text-[11px]'
              onChange={(event) => setTranslator(event.target.value)}
            />
          </label>

          <div className='grid gap-2 border-t border-border/70 pt-3'>
            <label className='flex items-center justify-between gap-3'>
              <span className='text-[11px]'>Deterministic</span>
              <Switch checked={deterministic} onCheckedChange={setDeterministic} />
            </label>
            <label className='flex items-center justify-between gap-3'>
              <span className='text-[11px]'>Overwrite</span>
              <Switch checked={overwrite} onCheckedChange={setOverwrite} />
            </label>
          </div>
        </div>

        <DialogFooter>
          <Button
            type='button'
            size='sm'
            className='h-8 text-[11px]'
            disabled={!ready}
            aria-busy={start.isPending}
            onClick={() => start.mutate()}
          >
            {start.isPending ? 'Starting…' : 'Start'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
