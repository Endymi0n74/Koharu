'use client'

import { useVirtualizer } from '@tanstack/react-virtual'
import { CheckIcon, ChevronDownIcon, SearchIcon } from 'lucide-react'
import { useCallback, useMemo, useRef, useState } from 'react'

import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import type { LlmCatalogModel, LlmProviderCatalog } from '@/lib/api/schemas'
import { cn } from '@/lib/utils'

const ITEM_HEIGHT = 32
const MAX_VISIBLE = 8

export type LlmModelOption = {
  model: LlmCatalogModel
  provider?: LlmProviderCatalog
}

type LlmModelSelectProps = {
  /** Stable key identifying the currently-selected model. */
  value?: string
  /** Flat list of local + provider-backed models. */
  options: LlmModelOption[]
  /** Map option → its value key. Must be deterministic. */
  getKey: (option: LlmModelOption) => string
  disabled?: boolean
  placeholder?: string
  className?: string
  triggerClassName?: string
  onChange: (key: string) => void
  'data-testid'?: string
}

/**
 * Model picker with a search input and a virtualized list.
 */
export function LlmModelSelect({
  value,
  options,
  getKey,
  disabled,
  placeholder,
  className,
  triggerClassName,
  onChange,
  ...props
}: LlmModelSelectProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return options
    return options.filter(({ model, provider }) => {
      const fields = [
        model.name,
        model.target.modelId,
        model.target.providerId,
        provider?.name,
        provider?.id,
      ]
      return fields.some((x) => x?.toLowerCase().includes(q))
    })
  }, [options, search])

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ITEM_HEIGHT,
    overscan: 4,
    enabled: open,
  })

  const viewportRef = useCallback(
    (node: HTMLDivElement | null) => {
      scrollRef.current = node
      if (node) virtualizer.measure()
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [open],
  )

  const selected = useMemo(() => options.find((o) => getKey(o) === value), [options, value, getKey])

  const listHeight = Math.min(Math.max(filtered.length, 1), MAX_VISIBLE) * ITEM_HEIGHT

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setSearch('')
      }}
    >
      <PopoverTrigger
        disabled={disabled}
        data-testid={props['data-testid']}
        className={cn(
          "flex h-7 w-full items-center justify-between gap-1.5 rounded-md border border-input bg-transparent px-2 py-1 text-xs whitespace-nowrap shadow-xs transition-colors outline-none hover:border-primary/40 hover:bg-primary/[0.03] focus-visible:border-primary/60 focus-visible:ring-[3px] focus-visible:ring-primary/25 disabled:cursor-not-allowed disabled:opacity-50 data-[state=open]:border-primary/60 data-[state=open]:ring-[3px] data-[state=open]:ring-primary/25 dark:bg-input/30 dark:hover:bg-input/50 [&_svg:not([class*='text-'])]:text-muted-foreground",
          triggerClassName,
        )}
      >
        <TriggerLabel selected={selected} placeholder={placeholder} />
        <ChevronDownIcon className='size-3.5 shrink-0 opacity-60' />
      </PopoverTrigger>
      <PopoverContent
        // Matches the enclosing LLM popover (w-64) — keep compact and
        // rely on the badge + short-name rules to stay legible.
        className={cn(
          'w-64 min-w-(--radix-popover-trigger-width) overflow-hidden border-primary/15 p-0 shadow-lg',
          className,
        )}
        align='start'
        onOpenAutoFocus={(e) => {
          e.preventDefault()
          inputRef.current?.focus()
        }}
      >
        <div className='relative border-b border-primary/10 bg-gradient-to-b from-primary/[0.04] to-transparent'>
          <SearchIcon className='pointer-events-none absolute top-1/2 left-2 size-3 -translate-y-1/2 text-muted-foreground' />
          <input
            ref={inputRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder='Search models…'
            className='w-full bg-transparent py-1.5 pr-2 pl-7 text-xs outline-none placeholder:text-muted-foreground/70'
          />
        </div>
        <ScrollArea className='relative' style={{ height: listHeight }} viewportRef={viewportRef}>
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((vi) => {
              const option = filtered[vi.index]
              const key = getKey(option)
              const isSelected = key === value
              return (
                <ModelRow
                  key={vi.key}
                  option={option}
                  selected={isSelected}
                  style={{ height: ITEM_HEIGHT, top: vi.start }}
                  onClick={() => {
                    onChange(key)
                    setOpen(false)
                    setSearch('')
                  }}
                />
              )
            })}
          </div>
        </ScrollArea>
        {filtered.length === 0 && (
          <div
            data-testid='llm-model-empty'
            className='px-2 py-6 text-center text-xs text-muted-foreground'
          >
            No models found
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}

/** Last path segment — strips vendor prefixes like `anthropic/claude-…`. */
function shortModelName(name: string): string {
  const idx = name.lastIndexOf('/')
  return idx >= 0 && idx < name.length - 1 ? name.slice(idx + 1) : name
}

/** Provider badge label. Collapse `openai-compatible` to a short `compat`. */
function providerBadgeLabel(provider: LlmProviderCatalog): string {
  if (provider.id === 'openai-compatible') return 'compat'
  return provider.name
}

function TriggerLabel({
  selected,
  placeholder,
}: {
  selected: LlmModelOption | undefined
  placeholder: string | undefined
}) {
  if (!selected) {
    return (
      <span className='truncate text-muted-foreground'>{placeholder ?? 'Select a model…'}</span>
    )
  }
  const { model, provider } = selected
  return (
    <span className='flex min-w-0 items-center gap-1.5' title={model.name}>
      {provider && <ProviderBadge label={providerBadgeLabel(provider)} />}
      <span className='truncate'>{shortModelName(model.name)}</span>
    </span>
  )
}

function ModelRow({
  option,
  selected,
  style,
  onClick,
}: {
  option: LlmModelOption
  selected: boolean
  style: React.CSSProperties
  onClick: () => void
}) {
  const { model, provider } = option
  return (
    <button
      type='button'
      title={model.name}
      className={cn(
        'absolute left-0 flex w-full cursor-default items-center gap-1.5 px-2 pr-7 text-left text-xs transition-colors select-none',
        selected
          ? 'bg-accent text-accent-foreground ring-1 ring-primary/30 ring-inset'
          : 'hover:bg-accent/60 hover:text-accent-foreground',
      )}
      style={style}
      onClick={onClick}
    >
      {provider && <ProviderBadge label={providerBadgeLabel(provider)} />}
      <span className='truncate'>{shortModelName(model.name)}</span>
      {selected && (
        <CheckIcon className='absolute top-1/2 right-2 size-3 -translate-y-1/2 text-primary' />
      )}
    </button>
  )
}

function ProviderBadge({ label }: { label: string }) {
  return (
    <span className='shrink-0 rounded-sm border border-primary/20 bg-primary/10 px-1 py-0.5 text-[9px] leading-none font-semibold tracking-wide text-primary uppercase'>
      {label}
    </span>
  )
}
