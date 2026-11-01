import type { Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installPersonalMock } from './personal-files-native'
const thumbnail =
  'data:image/png;base64,' +
  readFileSync(new URL('../../fixtures/preview.png', import.meta.url)).toString('base64')
export async function installSimilarMock(page: Page) {
  await installPersonalMock(page, false, true)
  await page.addInitScript(
    ({ thumbnail }) => {
      const native = (window as any).__TAURI_INTERNALS__,
        original = native.invoke
      const aliases: Record<string, string> = {
        get_similar_photos: 'get_duplicate_files',
        start_similar_scan: 'start_duplicate_scan',
        prepare_similar_trash: 'prepare_duplicate_trash',
        discard_similar_trash: 'discard_duplicate_trash',
        execute_similar_trash: 'execute_duplicate_trash',
      }
      const convert = (report: any) => {
        if (!report) return report
        const video = sessionStorage.getItem('video-candidates') === 'true'
        const items = report.items.map((i: any) => ({
          ...i,
          name: i.name.replace(/\.(txt|png|mp4)$/, video ? '.mp4' : '.png'),
          path: i.path.replace(/\.(txt|png|mp4)$/, video ? '.mp4' : '.png'),
        }))
        const { duplicates, ...base } = report
        return {
          ...base,
          items,
          similarPhotos: {
            method: video ? 'videoFramesVisionV2' : 'visionFeaturePrintV2',
            ...(video
              ? {
                  videos: items.map((i: any) => ({
                    id: i.id,
                    durationMs: 3000,
                    frames: [300, 1500, 2700].map((atMs) => ({
                      atMs: sessionStorage.getItem('video-bad-output') && atMs === 300 ? 0 : atMs,
                      thumbnail,
                    })),
                  })),
                }
              : {}),
            ...(sessionStorage.getItem('photo-sequences') === 'true'
              ? { sequence: { missingMetadata: 1 } }
              : {}),
            groups: [
              {
                id: '08a7a9f5-a95d-4a9e-a7b7-905a22309660',
                itemIds: items.map((i: any) => i.id),
                ...(sessionStorage.getItem('photo-sequences') === 'true'
                  ? { captureSpanMs: 600 }
                  : {}),
              },
            ],
            images: items.map((i: any, index: number) => ({
              id: i.id,
              width: 128 * (index + 1),
              height: 96 * (index + 1),
              thumbnail,
              ...(sessionStorage.getItem('photo-sequences') === 'true'
                ? { capturedAt: `2026:10:31 12:00:00.${100 + index * 300}+08:00` }
                : {}),
            })),
            checkedFiles: sessionStorage.getItem('photo-sequences') === 'true' ? 4 : 3,
            bytesRead: 196608,
          },
        }
      }
      native.invoke = async (command: string, args: any = {}) => {
        const calls = JSON.parse(sessionStorage.getItem('similar-calls') || '[]')
        sessionStorage.setItem('similar-calls', JSON.stringify([...calls, { command, args }]))
        if (command === 'start_similar_scan') {
          sessionStorage.setItem('photo-sequences', String(args.photoSequences === true))
          sessionStorage.setItem('video-candidates', String(args.videoCandidates === true))
        }
        if (command === 'get_similar_photos') {
          const value = await original(aliases[command], args)
          if (value.report) {
            value.report = convert(value.report)
            sessionStorage.setItem('personal-report', JSON.stringify(value.report))
          }
          return value
        }
        if (command === 'get_execution')
          return {
            ...(await original(command, args)),
            origin: 'similarPhotos',
            revision:
              sessionStorage.getItem('video-candidates') === 'true'
                ? 'MooMole.similar-videos.v1'
                : 'MooMole.similar-photos.v1',
          }
        if (command === 'preview_personal_file') {
          const report = JSON.parse(sessionStorage.getItem('personal-report')!),
            item = report.items.find((i: any) => i.id === args.itemId)
          if (!item || args.domain !== 'similarPhotos' || args.scanId !== report.id)
            throw Error('wrong image source')
          return {
            scanId: args.scanId,
            itemId: item.id,
            path: item.path,
            content: { kind: 'image', dataUrl: thumbnail, width: 128, height: 96, frames: 1 },
          }
        }
        return original(aliases[command] ?? command, args)
      }
    },
    { thumbnail },
  )
}
