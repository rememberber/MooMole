import type { Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installPersonalMock } from './personal-files-native'
const thumbnail =
  'data:image/png;base64,' +
  readFileSync(new URL('../../fixtures/preview.png', import.meta.url)).toString('base64')
export async function installSimilarMock(page: Page, raw = false) {
  await installPersonalMock(page, false, true)
  await page.addInitScript(
    ({ thumbnail, raw }) => {
      const rawRequests = new Set<string>()
      let releaseRaw: (() => void) | undefined
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
        const rawImages = raw && !video && sessionStorage.getItem('photo-sequences') !== 'true'
        const items = report.items.map((i: any, index: number) => ({
          ...i,
          name: i.name.replace(
            /\.(txt|png|mp4|dng|nef)$/,
            video ? '.mp4' : rawImages ? ['.dng', '.nef', '.png'][index] : '.png',
          ),
          path: i.path.replace(
            /\.(txt|png|mp4|dng|nef)$/,
            video ? '.mp4' : rawImages ? ['.dng', '.nef', '.png'][index] : '.png',
          ),
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
              ...(rawImages && index < 2
                ? { rawType: index === 0 ? 'com.adobe.raw-image' : 'com.nikon.raw-image' }
                : {}),
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
        if (command === 'cancel_task' && rawRequests.has(args.taskId)) {
          rawRequests.delete(args.taskId)
          return null
        }
        if (command === 'preview_raw_photo') {
          const report = JSON.parse(sessionStorage.getItem('personal-report')!),
            item = report.items.find((i: any) => i.id === args.itemId)
          if (
            !item ||
            args.domain !== 'similarPhotos' ||
            args.scanId !== report.id ||
            !args.requestId
          )
            throw Error('wrong RAW source')
          rawRequests.add(args.requestId)
          const value = {
            scanId: args.scanId,
            itemId: item.id,
            path: item.path,
            content: {
              kind: 'rawImage',
              dataUrl: thumbnail,
              width: 4928,
              height: 3264,
              typeIdentifier: item.path.endsWith('.nef')
                ? 'com.nikon.raw-image'
                : 'com.adobe.raw-image',
            },
          }
          const mode = sessionStorage.getItem('raw-preview-mode')
          if (mode === 'unsupported') {
            rawRequests.delete(args.requestId)
            throw Error('System RAW decoder does not support this sample')
          }
          if (mode === 'pending') {
            return new Promise((resolve) => {
              releaseRaw = () => resolve(value)
              Object.assign(window, { releaseOwnedRawPreview: () => releaseRaw?.() })
            })
          }
          rawRequests.delete(args.requestId)
          return mode === 'mismatch'
            ? { ...value, path: '/other.dng' }
            : mode === 'wrongKind'
              ? {
                  ...value,
                  content: { kind: 'image', dataUrl: thumbnail, width: 128, height: 96, frames: 1 },
                }
              : value
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
    { thumbnail, raw },
  )
}
