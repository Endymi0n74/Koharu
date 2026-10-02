'use client'

import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { call } from '@/lib/backend'
import { useKoharuStore } from '@/lib/store'
import { orderedLanguageChoices } from '@/lib/translation'
import { commands } from '@koharu/bridge/protocol'
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
  const [source, setSource] = useState<string | null>(null)
  const [output, setOutput] = useState<string | null>(null)
  const [lang, setLang] = useState<string | null>(null)
  const [deterministic, setDeterministic] = useState(true)
  const [overwrite, setOverwrite] = useState(false)
  const [busy, setBusy] = useState(false)

  const languageChoices = useMemo(
    () => orderedLanguageChoices(preferences?.languages ?? []),
    [preferences],
  )
  const target = lang ?? preferences?.pipeline.translation.target_language ?? 'fr-FR'
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
    void call(commands.startBatch, source, output, target, deterministic, overwrite)
      .then(() => onOpenChange(false))
      .catch(() => undefined)
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
