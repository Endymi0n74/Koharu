'use client'

import { CircleAlert, Download, Square, X } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { call } from '@/lib/backend'
import { commands, type Download as DownloadState, type Job } from '@/lib/protocol'
import { useKoharuStore } from '@/lib/store'
import { Button } from '@koharu/ui/components/button'

export function ActivityCenter() {
  const { t } = useTranslation()
  const jobs = useKoharuStore((state) => state.jobs)
  const downloads = useKoharuStore((state) => state.downloads)
  const visibleJobs = Object.values(jobs).filter(
    (job) => job.state === 'running' || job.state === 'failed',
  )
  const runningDownloads = Object.values(downloads).filter(
    (download) => download.state === 'running',
  )
  const failedDownloads = Object.values(downloads).filter((download) => download.state === 'failed')
  const visibleDownloads = [...runningDownloads, ...failedDownloads]
  if (visibleJobs.length === 0 && visibleDownloads.length === 0) return null

  return (
    <aside className='absolute right-3 bottom-9 z-30 flex w-72 max-w-[calc(100%-1.5rem)] flex-col rounded-2xl border border-border/50 bg-popover shadow-md'>
      <div className='border-b px-3 py-2'>
        <span className='text-[10px] font-semibold tracking-[0.1em] uppercase'>
          {t('activity.title')}
        </span>
      </div>
      {runningDownloads.length > 1 ? (
        <DownloadGroup downloads={runningDownloads} />
      ) : (
        runningDownloads.map((download) => <DownloadItem key={download.id} download={download} />)
      )}
      {failedDownloads.map((download) => (
        <DownloadItem key={download.id} download={download} />
      ))}
      {visibleJobs.map((job) => (
        <JobItem key={job.id} job={job} />
      ))}
    </aside>
  )
}

function DownloadGroup({ downloads }: { downloads: DownloadState[] }) {
  const { t } = useTranslation()
  const hasKnownTotal = downloads.every((download) => download.total > 0)
  const percent = hasKnownTotal
    ? progress(
        downloads.reduce((sum, download) => sum + download.completed, 0),
        downloads.reduce((sum, download) => sum + download.total, 0),
      )
    : null

  return (
    <div className='border-b p-3 last:border-b-0'>
      <div className='grid grid-cols-[1rem_minmax(0,1fr)_2.25rem_1.5rem] items-start gap-x-2.5'>
        <Download className='mt-0.5 size-3.5 justify-self-center text-primary' />
        <span className='truncate text-[11px]'>
          {t('activity.downloadingFiles', { count: downloads.length })}
        </span>
        <span className='text-right text-[10px] tabular-nums'>
          {percent !== null ? `${percent}%` : null}
        </span>
        <span aria-hidden='true' />
        <div className='col-start-2 col-end-4'>
          <Progress value={percent} />
        </div>
      </div>
    </div>
  )
}

function JobItem({ job }: { job: Job }) {
  const { t } = useTranslation()
  const dismiss = useKoharuStore((state) => state.dismissJob)
  if (job.state === 'failed') {
    return (
      <Failure
        message={job.error || t('activity.processingFailed')}
        onDismiss={() => dismiss(job.id)}
      />
    )
  }
  const percent = progress(job.completed, job.total)
  return (
    <div className='border-b p-3 last:border-b-0'>
      <div className='grid grid-cols-[1rem_minmax(0,1fr)_2.25rem_1.5rem] items-start gap-x-2.5'>
        <span className='mt-1.5 size-1.5 justify-self-center rounded-full bg-primary' />
        <div className='min-w-0'>
          <span className='block truncate text-[12px] font-medium capitalize'>
            {job.stage
              ? t(`phase.${job.stage}`, { defaultValue: job.stage })
              : t('activity.processing')}
          </span>
          <p className='mt-0.5 truncate text-[10px] text-muted-foreground'>{job.model}</p>
        </div>
        <span className='pt-0.5 text-right text-[10px] tabular-nums'>
          {percent !== null ? `${percent}%` : null}
        </span>
        <Button
          size='icon-xs'
          variant='ghost'
          className='-mt-1'
          aria-label={t('activity.stop')}
          onClick={() => void call(commands.stopJob, job.id).catch(() => undefined)}
        >
          <Square className='size-2.5 fill-current' />
        </Button>
        <div className='col-start-2 col-end-4'>
          <Progress value={percent} />
        </div>
      </div>
    </div>
  )
}

function DownloadItem({ download }: { download: DownloadState }) {
  const { t } = useTranslation()
  const dismiss = useKoharuStore((state) => state.dismissDownload)
  if (download.state === 'failed') {
    return (
      <Failure
        message={download.error || t('activity.downloadFailed')}
        onDismiss={() => dismiss(download.id)}
      />
    )
  }
  const percent = progress(download.completed, download.total)
  return (
    <div className='border-b p-3 last:border-b-0'>
      <div className='grid grid-cols-[1rem_minmax(0,1fr)_2.25rem_1.5rem] items-start gap-x-2.5'>
        <Download className='mt-0.5 size-3.5 justify-self-center text-primary' />
        <span className='truncate text-[11px]'>{download.name || t('activity.modelDownload')}</span>
        <span className='text-right text-[10px] tabular-nums'>
          {percent !== null ? `${percent}%` : null}
        </span>
        <span aria-hidden='true' />
        <div className='col-start-2 col-end-4'>
          <Progress value={percent} />
        </div>
      </div>
    </div>
  )
}

const PREVIEW_LIMIT = 160
const PREVIEW_HEAD = 96
const PREVIEW_TAIL = 64

/// Tokens marking the end of a model name in a GGUF filename: the
/// quantization / format part (e.g. `Q4_K_M`, `UD`, `F16`).
const QUANT_RE = /^(?:q|iq)\d|^(?:f(?:16|32|64|8)|bf16|ud)$/i

interface ModelHighlight {
  /// The model identifier, i.e. the leading tokens of the GGUF basename up to
  /// the quantization suffix (e.g. `gemma-4-E2B-it-qat`).
  prefix: string | null
  /// The GGUF filename (e.g. `gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf`).
  filename: string
}

/// Extracts the GGUF filename (and its model prefix) from a message line.
function findModelHighlight(line: string): ModelHighlight | null {
  const files = [...line.matchAll(/[^\\/\s]*\.gguf/gi)].map((match) => match[0])
  if (files.length === 0) return null
  const filename = files[files.length - 1]
  const tokens = filename.replace(/\.gguf$/i, '').split('-')
  const quantIndex = tokens.findIndex((token) => QUANT_RE.test(token))
  const prefix = (quantIndex > 0 ? tokens.slice(0, quantIndex) : tokens).join('-')
  return {
    filename,
    // Only highlight the prefix when it carries a model identifier; a bare
    // stem like `model` would otherwise match prose in the message.
    prefix: prefix && /\d/.test(prefix) ? prefix : null,
  }
}

/// Builds a compact preview for a failure message: the first line only, with a
/// middle ellipsis when the line is long (e.g. a full GGUF path). The GGUF
/// filename is kept whole so the failing model stays identifiable. Following
/// lines (e.g. llama.cpp's captured logs) are hidden behind the expand toggle.
function truncateMessage(message: string): string {
  const firstLine = message.split('\n')[0] ?? ''
  if (firstLine.length <= PREVIEW_LIMIT) return firstLine
  const head = firstLine.slice(0, PREVIEW_HEAD)
  const tail = firstLine.slice(truncationTailStart(firstLine))
  return `${head}…${tail}`
}

/// Start of the truncated tail: the last PREVIEW_TAIL characters, extended
/// backwards to the GGUF filename so the ellipsis never cuts it.
function truncationTailStart(line: string): number {
  const defaultStart = line.length - PREVIEW_TAIL
  const files = [...line.matchAll(/[^\\/\s]*\.gguf/gi)]
  const file = files[files.length - 1]
  if (!file) return defaultStart
  const start = file.index ?? 0
  const end = start + file[0].length
  // The default tail already shows the whole filename, or the filename starts
  // inside the head region: keep the default truncation.
  if (end <= defaultStart || start <= PREVIEW_HEAD) return defaultStart
  return start
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/// Renders text with the GGUF filename and model prefix highlighted.
function highlightText(text: string, highlight: ModelHighlight): ReactNode {
  const { filename, prefix } = highlight
  const patterns = prefix ? [filename, prefix] : [filename]
  const pattern = new RegExp(`(${patterns.map(escapeRegExp).join('|')})`, 'gi')
  const parts: ReactNode[] = []
  let last = 0
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0
    if (index > last) parts.push(text.slice(last, index))
    parts.push(
      <span key={index} className='font-semibold text-foreground'>
        {match[0]}
      </span>,
    )
    last = index + match[0].length
  }
  if (last < text.length) parts.push(text.slice(last))
  return <>{parts}</>
}

function Failure({ message, onDismiss }: { message: string; onDismiss: () => void }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const hasMore = message.includes('\n') || message.length > PREVIEW_LIMIT
  const firstLine = message.split('\n')[0] ?? ''
  const highlight = findModelHighlight(firstLine)
  const preview = truncateMessage(message)
  return (
    <div className='grid grid-cols-[1rem_minmax(0,1fr)_2.25rem_1.5rem] items-start gap-x-2.5 border-b p-3 text-[11px] last:border-b-0'>
      <CircleAlert className='mt-0.5 size-3.5 justify-self-center text-destructive' />
      <div className='col-start-2 col-end-4 min-w-0'>
        {expanded ? (
          <pre className='max-h-48 overflow-y-auto font-mono text-[10px] leading-4 break-words whitespace-pre-wrap text-destructive'>
            {message}
          </pre>
        ) : (
          <span className='block break-words text-destructive'>
            {highlight ? highlightText(preview, highlight) : preview}
          </span>
        )}
        {hasMore && (
          <button
            type='button'
            aria-expanded={expanded}
            className='mt-1 block font-medium text-destructive/80 hover:text-destructive'
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? t('activity.showLess') : t('activity.showMore')}
          </button>
        )}
      </div>
      <Button
        size='icon-xs'
        variant='ghost'
        className='-mt-1'
        aria-label={t('activity.dismiss')}
        onClick={onDismiss}
      >
        <X />
      </Button>
    </div>
  )
}

function Progress({ value }: { value: number | null }) {
  return (
    <div
      role='progressbar'
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value ?? undefined}
      className='mt-2 h-1 overflow-hidden rounded-full bg-muted'
    >
      <div
        className={`h-full rounded-full bg-primary ${value === null ? 'w-1/2' : ''}`}
        style={value === null ? undefined : { width: `${value}%` }}
      />
    </div>
  )
}

function progress(completed: number, total: number): number | null {
  return total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : null
}
