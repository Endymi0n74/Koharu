'use client'

import { LoaderCircleIcon, SparklesIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useCurrentPage } from '@/hooks/useCurrentPage'
import { startCodexImageGeneration, useGetCodexAuthStatus } from '@/lib/api/default/default'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useJobsStore } from '@/lib/stores/jobsStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function AiPanel() {
  const { t } = useTranslation()
  const page = useCurrentPage()
  const prompt = usePreferencesStore((s) => s.codexImagePrompt)
  const setPrompt = usePreferencesStore((s) => s.setCodexImagePrompt)
  const model = usePreferencesStore((s) => s.codexImageModel)
  const setModel = usePreferencesStore((s) => s.setCodexImageModel)
  const setShowRenderedImage = useEditorUiStore((s) => s.setShowRenderedImage)
  const showError = useEditorUiStore((s) => s.showError)
  const [busy, setBusy] = useState(false)
  const { data: auth } = useGetCodexAuthStatus()
  const isProcessing = useJobsStore((s) =>
    Object.values(s.jobs).some((job) => job.status === 'running'),
  )

  const signedIn = auth?.signedIn === true
  const promptReady = !!prompt?.trim()
  const modelValue = model?.trim() || 'gpt-5.5'
  const canGenerate = signedIn && !!page && promptReady && !isProcessing && !busy

  const handleGenerate = async () => {
    if (!signedIn || !page || !promptReady) return
    setBusy(true)
    try {
      setShowRenderedImage(true)
      await startCodexImageGeneration({
        pageId: page.id,
        prompt: prompt!.trim(),
        model: modelValue,
        quality: 'high',
      })
    } catch (err) {
      showError(String(err))
    } finally {
      setBusy(false)
    }
  }

  if (!signedIn) return null

  return (
    <div className='flex min-h-0 flex-col gap-3 text-xs'>
      <div className='space-y-1.5'>
        <Label className='text-[10px] font-semibold tracking-wide text-muted-foreground uppercase'>
          {t('ai.model')}
        </Label>
        <Input
          value={modelValue}
          onChange={(event) => setModel(event.target.value || undefined)}
          className='h-7 px-2 text-xs'
        />
      </div>

      <div className='space-y-1.5'>
        <Label className='text-[10px] font-semibold tracking-wide text-muted-foreground uppercase'>
          {t('ai.prompt')}
        </Label>
        <Textarea
          value={prompt ?? ''}
          onChange={(event) => setPrompt(event.target.value || undefined)}
          rows={8}
          className='min-h-36 resize-y px-2 py-1.5 text-xs leading-snug md:text-xs'
        />
      </div>

      <Button
        className='w-full gap-1.5'
        size='sm'
        disabled={!canGenerate}
        onClick={() => void handleGenerate()}
      >
        {busy || isProcessing ? (
          <LoaderCircleIcon className='size-3.5 animate-spin' />
        ) : (
          <SparklesIcon className='size-3.5' />
        )}
        {t('ai.generate')}
      </Button>

      {!page && <p className='text-xs text-muted-foreground'>{t('ai.noPage')}</p>}
    </div>
  )
}
