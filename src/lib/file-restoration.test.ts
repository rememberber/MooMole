import { expect, it } from 'vitest'
import { restorationCandidate } from './file-restoration'
it('software recovery candidates require a reliable ordinary-file receipt and never include cache or Library records', () => {
  const d = { origin: 'softwareContent', state: 'completed', method: 'trash' } as const,
    item = {
      state: 'moved',
      path: '/Users/demo/Downloads/owned.wav',
      receipt: { destination: '/Users/demo/.Trash/owned.wav', device: '1', inode: '2' },
    }
  expect(restorationCandidate(d, item)).toBe(true)
  for (const path of [
    '/Users/demo/Movies/owned.wav',
    '/Users/demo/ChatExport/files/owned.wav',
    '/Users/demo/Music/owned.wav',
  ])
    expect(restorationCandidate(d, { ...item, path })).toBe(true)
  for (const path of [
    '/Users/demo/Library/Mail Downloads/owned.wav',
    '/Users/demo/library/other.wav',
  ])
    expect(restorationCandidate(d, { ...item, path })).toBe(false)
  for (const patch of [
    { origin: 'mole' },
    { origin: 'fileRestoration' },
    { state: 'running' },
    { method: 'molePermanent' },
  ] as const)
    expect(restorationCandidate({ ...d, ...patch }, item)).toBe(false)
  for (const patch of [{ state: 'uncertain' }, { state: 'intent' }, { receipt: null }])
    expect(restorationCandidate(d, { ...item, ...patch })).toBe(false)
  expect(restorationCandidate({ ...d, origin: 'personalFiles', method: 'move' }, item)).toBe(true)
})
