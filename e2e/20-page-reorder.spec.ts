import {
  FIXTURE_IMAGE_PATHS,
  SMOKE_SET,
  addImages,
  bootstrapApp,
  importAndOpenPage,
  waitForNavigatorPageCount,
} from './helpers/app'
import { selectors } from './helpers/selectors'
import { expect, test } from './helpers/test'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('page manager button appears only when more than one page exists', async ({
  page,
}) => {
  const button = page.getByTestId(selectors.navigator.pageManagerButton)

  await importAndOpenPage(page, [SMOKE_SET[0]])
  await waitForNavigatorPageCount(page, 1)
  await expect(button).not.toBeVisible()

  await importAndOpenPage(page, SMOKE_SET)
  await waitForNavigatorPageCount(page, SMOKE_SET.length)
  await expect(button).toBeVisible()
})

test('page manager dialog opens and shows all pages', async ({ page }) => {
  await importAndOpenPage(page, SMOKE_SET)
  await waitForNavigatorPageCount(page, SMOKE_SET.length)

  const button = page.getByTestId(selectors.navigator.pageManagerButton)
  await button.click()

  const dialog = page.getByTestId(selectors.pageManager.dialog)
  await expect(dialog).toBeVisible()

  const grid = page.getByTestId(selectors.pageManager.grid)
  await expect(grid).toBeVisible()

  for (let i = 0; i < SMOKE_SET.length; i++) {
    await expect(page.getByTestId(selectors.pageManager.card(i))).toBeVisible()
  }

  const save = page.getByTestId(selectors.pageManager.save)
  await expect(save).toBeDisabled()
})

async function dragCard(
  page: import('@playwright/test').Page,
  source: import('@playwright/test').Locator,
  target: import('@playwright/test').Locator,
) {
  const sourceBox = await source.boundingBox()
  const targetBox = await target.boundingBox()
  if (!sourceBox || !targetBox) throw new Error('Card bounding box not found')
  await page.mouse.move(
    sourceBox.x + sourceBox.width / 2,
    sourceBox.y + sourceBox.height / 2,
  )
  await page.mouse.down()
  await page.mouse.move(
    targetBox.x + targetBox.width / 2,
    targetBox.y + targetBox.height / 2,
    { steps: 15 },
  )
  await page.mouse.up()
  // dnd-kit PointerSensor blocks click events for 50 ms after drag end
  await page.waitForTimeout(60)
}

test('drag-and-drop reorders pages and persists after save', async ({
  page,
}) => {
  await importAndOpenPage(page, SMOKE_SET)
  await waitForNavigatorPageCount(page, SMOKE_SET.length)

  const panel = page.getByTestId(selectors.navigator.panel)
  const navigatorPages = panel.locator('[data-page-index]')
  await expect(navigatorPages).toHaveCount(SMOKE_SET.length)

  const originalOrder = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )

  const button = page.getByTestId(selectors.navigator.pageManagerButton)
  await button.click()
  const dialog = page.getByTestId(selectors.pageManager.dialog)
  await expect(dialog).toBeVisible()

  const firstCard = page.getByTestId(selectors.pageManager.card(0))
  const lastCard = page.getByTestId(
    selectors.pageManager.card(SMOKE_SET.length - 1),
  )
  await dragCard(page, firstCard, lastCard)

  const save = page.getByTestId(selectors.pageManager.save)
  await expect(save).toBeEnabled()
  await save.click()

  await expect(dialog).not.toBeVisible()

  const newOrder = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  expect(newOrder).not.toEqual(originalOrder)
})

test('cancel discards reorder changes', async ({ page }) => {
  await importAndOpenPage(page, SMOKE_SET)
  await waitForNavigatorPageCount(page, SMOKE_SET.length)

  const panel = page.getByTestId(selectors.navigator.panel)
  const navigatorPages = panel.locator('[data-page-index]')
  await expect(navigatorPages).toHaveCount(SMOKE_SET.length)

  const originalOrder = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )

  const button = page.getByTestId(selectors.navigator.pageManagerButton)
  await button.click()
  const dialog = page.getByTestId(selectors.pageManager.dialog)
  await expect(dialog).toBeVisible()

  const firstCard = page.getByTestId(selectors.pageManager.card(0))
  const lastCard = page.getByTestId(
    selectors.pageManager.card(SMOKE_SET.length - 1),
  )
  await dragCard(page, firstCard, lastCard)

  await dialog.getByRole('button', { name: /cancel/i }).click()
  await expect(dialog).not.toBeVisible()

  const unchangedOrder = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  expect(unchangedOrder).toEqual(originalOrder)
})

test('drag from second row reorders correctly', async ({ page }) => {
  await importAndOpenPage(page, FIXTURE_IMAGE_PATHS)
  await waitForNavigatorPageCount(page, FIXTURE_IMAGE_PATHS.length)

  const panel = page.getByTestId(selectors.navigator.panel)
  const navigatorPages = panel.locator('[data-page-index]')
  await expect(navigatorPages).toHaveCount(FIXTURE_IMAGE_PATHS.length)

  const originalOrder = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )

  const button = page.getByTestId(selectors.navigator.pageManagerButton)
  await button.click()
  const dialog = page.getByTestId(selectors.pageManager.dialog)
  await expect(dialog).toBeVisible()

  const lastCard = page.getByTestId(
    selectors.pageManager.card(FIXTURE_IMAGE_PATHS.length - 1),
  )
  await lastCard.scrollIntoViewIfNeeded()

  const firstCard = page.getByTestId(selectors.pageManager.card(0))
  await dragCard(page, lastCard, firstCard)

  const save = page.getByTestId(selectors.pageManager.save)
  await expect(save).toBeEnabled()
  await save.click()

  await expect(dialog).not.toBeVisible()

  const newOrder = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  expect(newOrder).not.toEqual(originalOrder)
})

test('reordered pages keep their order after adding new images', async ({
  page,
}) => {
  // ── Step 1: Import Image A and Image B ────────────────────────
  const [imageA, imageB] = FIXTURE_IMAGE_PATHS.slice(0, 2)
  await importAndOpenPage(page, [imageA, imageB])
  await waitForNavigatorPageCount(page, 2)

  const panel = page.getByTestId(selectors.navigator.panel)
  const navigatorPages = panel.locator('[data-page-index]')

  const [srcA, srcB] = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )

  // ── Step 2: Open Page Manager, reorder to [B, A] ─────────────
  await page.getByTestId(selectors.navigator.pageManagerButton).click()
  const dialog = page.getByTestId(selectors.pageManager.dialog)
  await expect(dialog).toBeVisible()

  await dragCard(
    page,
    page.getByTestId(selectors.pageManager.card(0)),
    page.getByTestId(selectors.pageManager.card(1)),
  )
  const save = page.getByTestId(selectors.pageManager.save)
  await expect(save).toBeEnabled()
  await save.click()
  await expect(dialog).not.toBeVisible()

  // Verify, navigator shows [B, A]
  let order = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  expect(order).toEqual([srcB, srcA])

  // ── Step 3: Add Image C via "Add File" ────────────────────────
  const [imageC] = FIXTURE_IMAGE_PATHS.slice(2, 3)
  await addImages(page, [imageC])
  await waitForNavigatorPageCount(page, 3)

  order = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  expect(order[0]).toBe(srcB)
  expect(order[1]).toBe(srcA)
  const srcC = order[2]

  // ── Step 4: Open Page Manager, move C to first position ───────
  await page.getByTestId(selectors.navigator.pageManagerButton).click()
  await expect(dialog).toBeVisible()

  await dragCard(
    page,
    page.getByTestId(selectors.pageManager.card(2)),
    page.getByTestId(selectors.pageManager.card(0)),
  )
  await expect(save).toBeEnabled()
  await save.click()
  await expect(dialog).not.toBeVisible()

  // Verify, navigator shows [C, B, A]
  order = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  expect(order).toEqual([srcC, srcB, srcA])

  // ── Step 5: Add Image D via "Add File" ────────────────────────
  const [imageD] = FIXTURE_IMAGE_PATHS.slice(3, 4)
  await addImages(page, [imageD])
  await waitForNavigatorPageCount(page, 4)

  order = await navigatorPages.evaluateAll((els) =>
    els.map((el) => el.querySelector('img')?.src ?? ''),
  )
  const srcD = order[3]

  expect(order).toEqual([srcC, srcB, srcA, srcD])
})
