'use client'

import { useEffect, useState } from 'react'
import type { Update } from '@tauri-apps/plugin-updater'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Trans, useTranslation } from 'react-i18next'
import { Download, RefreshCw, AlertCircle } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'

type Phase =
  | { kind: 'hidden' }
  | { kind: 'prompt'; update: Update }
  | {
      kind: 'downloading'
      update: Update
      downloaded: number
      total: number | null
    }
  | { kind: 'error'; message: string; retry: () => Promise<void> }

export default function Updater() {
  const [phase, setPhase] = useState<Phase>({ kind: 'hidden' })

  const runCheck = async () => {
    try {
      const update = await check()
      if (update) setPhase({ kind: 'prompt', update })
    } catch (err) {
      console.warn('[updater] check failed', err)
      setPhase({ kind: 'error', message: String(err), retry: runCheck })
    }
  }

  const runInstall = async (update: Update) => {
    setPhase({ kind: 'downloading', update, downloaded: 0, total: null })
    try {
      await update.downloadAndInstall((event) => {
        setPhase((prev) => {
          if (prev.kind !== 'downloading') return prev
          if (event.event === 'Started')
            return { ...prev, total: event.data.contentLength ?? null }
          if (event.event === 'Progress')
            return {
              ...prev,
              downloaded: prev.downloaded + event.data.chunkLength,
            }
          return prev
        })
      })
      await relaunch()
    } catch (err) {
      console.warn('[updater] install failed', err)
      setPhase({
        kind: 'error',
        message: String(err),
        retry: () => runInstall(update),
      })
    }
  }

  useEffect(() => {
    void runCheck()
  }, [])

  const close = () => setPhase({ kind: 'hidden' })

  return (
    <Dialog open={phase.kind !== 'hidden'} onOpenChange={(o) => !o && close()}>
      <DialogContent className='flex w-[520px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0'>
        {phase.kind === 'prompt' && (
          <PromptView
            update={phase.update}
            onLater={close}
            onUpdate={() => runInstall(phase.update)}
          />
        )}
        {phase.kind === 'downloading' && (
          <DownloadingView
            version={phase.update.version}
            downloaded={phase.downloaded}
            total={phase.total}
          />
        )}
        {phase.kind === 'error' && (
          <ErrorView
            message={phase.message}
            onRetry={phase.retry}
            onClose={close}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function PromptView({
  update,
  onLater,
  onUpdate,
}: {
  update: Update
  onLater: () => void
  onUpdate: () => void
}) {
  const { t } = useTranslation()
  return (
    <>
      <header className='flex items-center gap-3 px-6 pt-6 pb-4'>
        <div className='bg-primary/10 text-primary flex size-10 items-center justify-center rounded-full'>
          <Download className='size-5' />
        </div>
        <div className='flex flex-col gap-0.5'>
          <DialogTitle className='text-base'>
            {t('updater.available.title')}
          </DialogTitle>
          <DialogDescription>
            <Trans
              i18nKey='updater.available.description'
              values={{ version: update.version }}
              components={{
                strong: <span className='text-foreground font-medium' />,
              }}
            />
          </DialogDescription>
        </div>
      </header>
      <Separator />
      {update.body ? (
        <ScrollArea className='h-64'>
          <div className='prose prose-sm dark:prose-invert [&_a]:text-primary [&_h3]:text-muted-foreground max-w-none px-6 py-4 [&_h2]:mt-4 [&_h2]:mb-2 [&_h2]:text-sm [&_h2]:font-semibold [&_h3]:mt-3 [&_h3]:mb-1 [&_h3]:text-xs [&_h3]:font-semibold [&_h3]:tracking-wide [&_h3]:uppercase [&_li]:my-0.5 [&_p]:my-1.5 [&_ul]:my-1.5 [&_ul]:list-disc [&_ul]:pl-5'>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {update.body}
            </ReactMarkdown>
          </div>
        </ScrollArea>
      ) : (
        <div className='text-muted-foreground px-6 py-6 text-sm'>
          {t('updater.noNotes')}
        </div>
      )}
      <Separator />
      <footer className='flex justify-end gap-2 px-6 py-4'>
        <Button variant='outline' onClick={onLater}>
          {t('updater.later')}
        </Button>
        <Button onClick={onUpdate}>
          <Download className='size-4' />
          {t('updater.update')}
        </Button>
      </footer>
    </>
  )
}

function DownloadingView({
  version,
  downloaded,
  total,
}: {
  version: string
  downloaded: number
  total: number | null
}) {
  const { t } = useTranslation()
  const percent = total ? Math.min(100, (downloaded / total) * 100) : null

  return (
    <div className='flex flex-col gap-4 p-6'>
      <div className='flex items-center gap-3'>
        <div className='bg-primary/10 text-primary flex size-10 items-center justify-center rounded-full'>
          <Download className='size-5 animate-pulse' />
        </div>
        <div className='flex flex-col gap-0.5'>
          <DialogTitle className='text-base'>
            {t('updater.downloading.title')}
          </DialogTitle>
          <DialogDescription>
            {t('updater.downloading.subtitle', { version })}
          </DialogDescription>
        </div>
      </div>
      <div className='space-y-2'>
        <Progress value={percent ?? undefined} />
        <div className='text-muted-foreground flex justify-between text-xs tabular-nums'>
          <span>
            {formatBytes(downloaded)}
            {total ? ` / ${formatBytes(total)}` : ''}
          </span>
          {percent != null && <span>{percent.toFixed(0)}%</span>}
        </div>
      </div>
    </div>
  )
}

function ErrorView({
  message,
  onRetry,
  onClose,
}: {
  message: string
  onRetry: () => Promise<void>
  onClose: () => void
}) {
  const { t } = useTranslation()
  return (
    <>
      <header className='flex items-center gap-3 px-6 pt-6 pb-4'>
        <div className='bg-destructive/10 text-destructive flex size-10 items-center justify-center rounded-full'>
          <AlertCircle className='size-5' />
        </div>
        <div className='flex flex-col gap-0.5'>
          <DialogTitle className='text-base'>
            {t('updater.error.title')}
          </DialogTitle>
          <DialogDescription className='break-words'>
            {t('updater.error.description')}
          </DialogDescription>
        </div>
      </header>
      <Separator />
      <ScrollArea className='max-h-40'>
        <pre className='text-muted-foreground px-6 py-4 text-xs break-words whitespace-pre-wrap'>
          {message}
        </pre>
      </ScrollArea>
      <Separator />
      <footer className='flex justify-end gap-2 px-6 py-4'>
        <Button variant='outline' onClick={onClose}>
          {t('updater.close')}
        </Button>
        <Button onClick={() => onRetry()}>
          <RefreshCw className='size-4' />
          {t('updater.retry')}
        </Button>
      </footer>
    </>
  )
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
