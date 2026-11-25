'use client'

import { useMutation } from '@tanstack/react-query'
import { ChevronDown } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Choice } from '@/components/controls/Choice'
import { ModelPicker } from '@/components/controls/ModelPicker'
import { useModels, usePreferences } from '@/lib/queries'
import { languageName, languageNames } from '@/lib/translation'
import { vendorName } from '@/lib/vendors'
import { Channel, commands, type Model } from '@koharu/ipc/protocol'
import { Badge } from '@koharu/ui/components/badge'
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
import { Popover, PopoverContent, PopoverTrigger } from '@koharu/ui/components/popover'
import { toast } from '@koharu/ui/components/toast'

/** A page stage's line, and the run's total, as `koharu-batch` prints them. */
const STAGE = /^\w+ done on \S+$/
const QUEUED = /^(\d+) page stages to run$/

/** Lets the user pick a source folder, an output project, and options, then
 * runs `koharu-batch` over the folder. */
export function BatchDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t, i18n } = useTranslation()
  const preferences = usePreferences().data
  const models = useModels()
  const [source, setSource] = useState<string | null>(null)
  const [output, setOutput] = useState<string | null>(null)
  const [language, setLanguage] = useState<string | null>(null)
  const [translator, setTranslator] = useState('')
  const [modelOpen, setModelOpen] = useState(false)
  const [steps, setSteps] = useState<{ done: number; total: number } | null>(null)

  // A folder cannot be both the source and the output: a state restored from
  // an earlier pick may hold the same one twice.
  useEffect(() => {
    if (open && source && output && source === output) setOutput(null)
  }, [open, source, output])

  // The saved target, so the dialog and the editor translate into the same
  // language; a fresh install starts on the CLI's own French default.
  const target = language ?? preferences?.pipeline.translator?.target ?? 'fr-FR'
  // A saved target can be a tag the list doesn't offer, as the CLI's own `fr-FR` default: it
  // reads by its language's name, and the tag itself is still what a run is given.
  const languages = useMemo(() => {
    const names = languageNames(i18n.language)
    if (target in names) return names
    const language = target.split('-')[0]
    return { ...names, [target]: language ? languageName(language, i18n.language) : target }
  }, [i18n.language, target])
  // `koharu-batch` picks its model when no `--translator` is given, which is
  // the dialog's Auto: a synthetic first entry, as the fork's menu had.
  const auto: Model = {
    reference: '',
    name: t('batch.modelAuto'),
    vendor: 'local',
    images: false,
    tools: false,
  }
  const picked = translator ? models.data?.find((model) => model.reference === translator) : auto

  // A failed start shows its reason through the mutation cache's toast; a run
  // that ends opens the project it wrote, then fires the done toast so the user
  // sees it after the editor switches to the new project.
  const start = useMutation({
    mutationFn: () => {
      setSteps(null)
      let done = 0
      let total = 0
      const channel = new Channel<string>((line) => {
        const queued = QUEUED.exec(line)
        if (queued) total = Number(queued[1])
        else if (STAGE.test(line)) done += 1
        else return
        setSteps({ done, total })
      })
      return commands.startBatch(source ?? '', output ?? '', target, translator || null, channel)
    },
    onSuccess: async (projectPath) => {
      if (projectPath && typeof projectPath === 'string') {
        await commands.openProjectPath(projectPath)
      }
      toast.add({ type: 'success', title: t('batch.done'), description: output ?? undefined })
      onOpenChange(false)
    },
  })

  const ready = Boolean(source && output && source !== output) && !start.isPending

  // A failed pick shows its reason through the mutation cache's toast.
  const pickSource = useMutation({
    mutationFn: (setPath: (path: string) => void) =>
      commands.pickBatchFolder().then((path) => {
        if (!path) return
        if (path === output) throw new Error(t('batch.sameFolder'))
        setPath(path)
      }),
  })

  // `koharu-batch` writes the project into the chosen folder; when one is
  // already there, `start_batch` backs it up and passes `--overwrite`.
  const pickOutput = useMutation({
    mutationFn: (setPath: (path: string) => void) =>
      commands.pickBatchOutput().then((path) => {
        if (!path) return
        if (path === source) throw new Error(t('batch.sameFolder'))
        setPath(path)
      }),
  })

  const folderRow = (
    label: string,
    path: string | null,
    setPath: (path: string) => void,
    pick: (setPath: (path: string) => void) => void,
  ) => (
    <label className='grid gap-1.5'>
      <span className='text-[11px] font-medium text-muted-foreground'>{label}</span>
      <span className='flex gap-2'>
        <Input
          readOnly
          value={path ?? ''}
          placeholder={t('batch.noneSelected')}
          className='h-8 flex-1 text-[11px]'
        />
        <Button
          type='button'
          variant='outline'
          size='sm'
          className='h-8 shrink-0 text-[11px]'
          onClick={() => pick(setPath)}
        >
          {t('batch.choose')}
        </Button>
      </span>
    </label>
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='max-w-md gap-4 p-5'>
        <DialogHeader className='gap-1'>
          <DialogTitle className='text-[15px]'>{t('batch.title')}</DialogTitle>
          <DialogDescription className='text-[11px]'>{t('batch.description')}</DialogDescription>
        </DialogHeader>

        <div className='grid gap-3'>
          {folderRow(t('batch.folder'), source, setSource, pickSource.mutate)}
          {folderRow(t('batch.output'), output, setOutput, pickOutput.mutate)}

          <label className='grid gap-1.5'>
            <span className='text-[11px] font-medium text-muted-foreground'>
              {t('model.targetLanguage')}
            </span>
            <Choice
              value={target}
              choices={languages}
              label={t('model.targetLanguage')}
              onChange={setLanguage}
            />
          </label>

          <div className='grid gap-1.5'>
            <span className='text-[11px] font-medium text-muted-foreground'>
              {t('batch.model')}
            </span>
            <Popover open={modelOpen} onOpenChange={setModelOpen}>
              <PopoverTrigger
                type='button'
                aria-label={t('batch.model')}
                className='flex h-8 w-full min-w-0 items-center justify-between gap-2 rounded-lg border border-input bg-transparent px-2.5 text-[11px] transition-colors outline-none hover:bg-foreground/[0.03] focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50'
              >
                <span className='flex min-w-0 flex-1 items-center gap-2 text-left'>
                  {picked && (
                    <Badge
                      variant='outline'
                      className='shrink-0 px-1.5 py-0 text-[9px] font-medium'
                    >
                      {vendorName(picked.vendor, t)}
                    </Badge>
                  )}
                  <span className='truncate'>{picked?.name ?? translator}</span>
                </span>
                <ChevronDown className='size-3.5 shrink-0 text-muted-foreground' />
              </PopoverTrigger>
              <PopoverContent
                align='start'
                sideOffset={4}
                className='w-(--anchor-width) min-w-64 gap-0 overflow-hidden rounded-xl border border-border/50 p-1 shadow-sm ring-0'
              >
                <ModelPicker
                  value={translator}
                  models={[auto, ...(models.data ?? [])]}
                  onBack={() => setModelOpen(false)}
                  onSelect={(reference) => {
                    setTranslator(reference)
                    setModelOpen(false)
                  }}
                />
              </PopoverContent>
            </Popover>
          </div>

          {start.isPending && steps ? (
            <p className='text-[11px] text-muted-foreground'>
              {t('batch.progress', { done: steps.done, total: steps.total })}
            </p>
          ) : null}
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
            {start.isPending ? t('batch.starting') : t('batch.start')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
