'use client'

import { ChevronDown } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ModelPicker } from '@/components/controls/ModelPicker'
import { call, refreshTranslationModels } from '@/lib/backend'
import { useKoharuStore } from '@/lib/store'
import { modelKey, orderedLanguageChoices, providerName } from '@/lib/translation'
import { commands } from '@koharu/bridge/protocol'
import type { Model, ModelSelection } from '@koharu/bridge/protocol'
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'
import { Switch } from '@koharu/ui/components/switch'

/// The picker's starting choice: no explicit model, so `koharu-batch` picks a
/// local model from the free VRAM, exactly like its own default run.
const AUTO_SELECTION: ModelSelection = {
  provider: 'local',
  model: null,
  quantization: null,
  vision: true,
  reasoning: false,
}

/// Launches a `koharu-batch` run over a whole folder: the child process does
/// the translation, and the job it reports shows up in the activity center.
export function BatchDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const preferences = useKoharuStore((state) => state.preferences)
  const translationModels = useKoharuStore((state) => state.translationModels)
  const [source, setSource] = useState<string | null>(null)
  const [output, setOutput] = useState<string | null>(null)
  const [lang, setLang] = useState<string | null>(null)
  const [model, setModel] = useState<ModelSelection>(AUTO_SELECTION)
  const [modelOpen, setModelOpen] = useState(false)
  const [deterministic, setDeterministic] = useState(true)
  const [overwrite, setOverwrite] = useState(false)
  const [busy, setBusy] = useState(false)
  const [startError, setStartError] = useState<string | null>(null)

  // A fresh attempt starts from a clean slate, including after a reopen, and
  // the catalog is refreshed so hosted providers show up even after an edit
  // in the settings screen.
  useEffect(() => {
    if (open) {
      setStartError(null)
      void refreshTranslationModels().catch(() => undefined)
    }
  }, [open])

  const languageChoices = useMemo(
    () => orderedLanguageChoices(preferences?.languages ?? []),
    [preferences],
  )
  const providers = preferences?.providers.entries ?? []
  const target = lang ?? preferences?.pipeline.translation.target_language ?? 'fr-FR'
  const auto = model.provider === 'local' && !model.model
  const picked = translationModels.find((candidate) => modelKey(candidate) === modelKey(model))
  const quantization =
    picked?.quantizations.find((entry) => entry.id === model.quantization)?.name ??
    model.quantization
  // Synthetic first entry: choosing it restores the CLI's own auto pick.
  const autoModel: Model = {
    provider: 'local',
    model: null,
    name: t('batch.autoModel'),
    quantizations: [],
    vision: true,
    reasoning: false,
  }
  const ready = Boolean(source && output) && !busy

  const pick = (setPath: (path: string) => void) =>
    void call(commands.pickBatchFolder)
      .then((path) => {
        if (path) setPath(path)
      })
      .catch(() => undefined)

  const start = () => {
    if (!source || !output) return
    setBusy(true)
    setStartError(null)
    void call(
      commands.startBatch,
      source,
      output,
      target,
      model,
      deterministic,
      overwrite,
    )
      .then(() => onOpenChange(false))
      .catch((error: unknown) => {
        setStartError(error instanceof Error ? error.message : String(error))
      })
      .finally(() => setBusy(false))
  }

  const folderRow = (label: string, path: string | null, setPath: (path: string) => void) => (
    <label className='grid gap-1.5'>
      <span className='text-[11px] font-medium text-muted-foreground'>{label}</span>
      <span className='flex gap-2'>
        <Input
          readOnly
          value={path ?? ''}
          placeholder={t('batch.pick')}
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
          {folderRow(t('batch.source'), source, setSource)}
          {folderRow(t('batch.output'), output, setOutput)}

          <label className='grid gap-1.5'>
            <span className='text-[11px] font-medium text-muted-foreground'>
              {t('model.targetLanguage')}
            </span>
            <Select
              value={target}
              items={Object.fromEntries(
                languageChoices.map((language) => [language.tag, language.name]),
              )}
              onValueChange={(tag) => tag && setLang(tag)}
            >
              <SelectTrigger aria-label={t('model.targetLanguage')} className='h-8 w-full text-[11px]'>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {languageChoices.map((language) => (
                  <SelectItem key={language.tag} value={language.tag}>
                    {language.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
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
                <span className='flex min-w-0 flex-1 items-center gap-2'>
                  <Badge variant='outline' className='shrink-0 px-1.5 py-0 text-[9px] font-medium'>
                    {providerName(providers, model.provider)}
                  </Badge>
                  <span className='truncate'>
                    {auto
                      ? t('batch.autoModel')
                      : (picked?.name ?? model.model ?? providerName(providers, model.provider))}
                    {!auto && quantization && (
                      <span className='text-muted-foreground'> · {quantization}</span>
                    )}
                  </span>
                </span>
                <ChevronDown className='size-3.5 shrink-0 text-muted-foreground' />
              </PopoverTrigger>
              <PopoverContent
                align='start'
                sideOffset={4}
                className='w-(--anchor-width) min-w-64 gap-0 overflow-hidden rounded-xl border border-border/50 p-1 shadow-sm ring-0'
              >
                <ModelPicker
                  value={model}
                  models={[autoModel, ...translationModels]}
                  providers={providers}
                  onBack={() => setModelOpen(false)}
                  onSelect={(selection) => {
                    setModel(selection)
                    setModelOpen(false)
                  }}
                />
              </PopoverContent>
            </Popover>
          </div>

          <div className='grid gap-2 border-t border-border/70 pt-3'>
            <label className='flex items-center justify-between gap-3'>
              <span className='text-[11px]'>{t('batch.deterministic')}</span>
              <Switch checked={deterministic} onCheckedChange={setDeterministic} />
            </label>
            <label className='flex items-center justify-between gap-3'>
              <span className='text-[11px]'>{t('batch.overwrite')}</span>
              <Switch checked={overwrite} onCheckedChange={setOverwrite} />
            </label>
          </div>
        </div>

        {startError && (
          <p role='alert' className='text-[11px] text-destructive'>
            {startError}
          </p>
        )}

        <DialogFooter>
          <Button
            type='button'
            size='sm'
            className='h-8 text-[11px]'
            disabled={!ready}
            aria-busy={busy}
            onClick={start}
          >
            {busy ? t('batch.starting') : t('batch.start')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
