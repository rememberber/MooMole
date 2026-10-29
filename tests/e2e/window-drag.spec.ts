import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

test('a stalled native gesture moves only the main window; controls and release never start another move', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  await page.addInitScript(() => {
    const native = (window as any).__TAURI_INTERNALS__
    const invoke = native.invoke
    native.metadata = { currentWindow: { label: 'main' } }
    native.invoke = async (command: string, args: any) => {
      if (command === 'plugin:window|outer_position')
        return JSON.parse(sessionStorage.getItem('drag-position') ?? '{"x":100,"y":200}')
      if (command === 'plugin:window|scale_factor') return 2
      if (command === 'plugin:window|set_position') {
        if (args.label !== 'main') throw new Error('Unexpected window')
        sessionStorage.setItem(
          'drag-position',
          JSON.stringify(JSON.parse(JSON.stringify(args.value)).Physical),
        )
        sessionStorage.setItem(
          'drag-moves',
          String(Number(sessionStorage.getItem('drag-moves')) + 1),
        )
        return
      }
      return invoke(command, args)
    }
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const toolbar = page.getByRole('banner', { name: '概览工具栏' })
  const box = (await toolbar.boundingBox())!
  await page.mouse.move(box.x + box.width * 0.65, box.y + 35)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width * 0.65 + 40, box.y + 55, { steps: 8 })
  await expect
    .poll(async () => page.evaluate(() => sessionStorage.getItem('drag-position')))
    .toBe('{"x":180,"y":240}')
  await page.mouse.up()
  const moves = await page.evaluate(() => sessionStorage.getItem('drag-moves'))
  await page.mouse.move(box.x + 200, box.y + 100)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  const input = page.getByRole('textbox', { name: '扫描目录' })
  await input.fill('/fixture/editable')
  await input.dispatchEvent('mousedown', { button: 0, isPrimary: true, pointerId: 1 })
  await page
    .getByRole('button', { name: '开始分析', exact: true })
    .dispatchEvent('mousedown', { button: 0, isPrimary: true, pointerId: 1 })
  expect(await page.evaluate(() => sessionStorage.getItem('drag-moves'))).toBe(moves)
  await expect(input).toHaveValue('/fixture/editable')
})
