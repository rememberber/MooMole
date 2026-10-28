import { maintenanceItems } from '../lib/maintenance'
import { filterTasks } from '../lib/task-history'
import type { TaskRecord } from '../types'
import { afterEach, describe, expect, it } from 'vitest'
import { resolveLanguage, setLanguagePreference, getLanguage, getLocale } from './locale'
import { translate } from './translate'
import messages from './messages.json'
import { jsx, jsxs } from './jsx-runtime'

afterEach(() => setLanguagePreference('system'))
describe('language resolution', () => {
  it.each([
    ['system', 'zh-CN', 'zh'],
    ['system', 'zh-Hant-TW', 'zh'],
    ['system', 'en-GB', 'en'],
    ['system', 'ja-JP', 'ja'],
    ['system', 'de-DE', 'en'],
    ['system', '', 'en'],
    [undefined, 'ja_JP', 'ja'],
    ['ja', 'zh-CN', 'ja'],
    ['zh', 'en-US', 'zh'],
    ['invalid', 'fr-FR', 'en'],
  ])('resolves %s / %s to %s', (preference, system, expected) => {
    expect(resolveLanguage(preference, system)).toBe(expected)
  })
  it('applies locale formatting with the selected language', () => {
    setLanguagePreference('ja')
    expect(getLanguage()).toBe('ja')
    expect(getLocale()).toBe('ja-JP')
  })
})
describe('presentation translations', () => {
  it('has complete, nonempty translations with matching placeholders', () => {
    const seen = new Set<string>()
    for (const [source, en, ja] of messages) {
      expect(seen.has(source), source).toBe(false)
      seen.add(source)
      expect(en.trim(), source).not.toBe('')
      expect(ja.trim(), source).not.toBe('')
      const placeholders = (text: string) => [...text.matchAll(/\{\d+\}/g)].map((m) => m[0]).sort()
      expect(placeholders(en), source).toEqual(placeholders(source))
      expect(placeholders(ja), source).toEqual(placeholders(source))
    }
  })
  it('translates dynamic messages without changing paths', () => {
    setLanguagePreference('en')
    expect(translate('空间分析')).toBe('Space analyzer')
    expect(translate('审阅 /Users/demo/文稿')).toBe('Review /Users/demo/文稿')
    expect(translate('审阅 清理')).toBe('Review 清理')
    expect(translate('审阅 /Users/demo/Two  Spaces/清理')).toBe(
      'Review /Users/demo/Two  Spaces/清理',
    )
    expect(translate('在 Finder 中显示 清理')).toBe('Show 清理 in Finder')
    expect(translate('/Users/demo/清理')).toBe('/Users/demo/清理')
    setLanguagePreference('ja')
    expect(translate('空间分析')).toBe('容量分析')
    setLanguagePreference('zh')
    expect(translate('空间分析')).toBe('空间分析')
  })
  it('keeps element identity, selection values and original evidence intact', () => {
    setLanguagePreference('en')
    const props = { children: '清理', value: '清理', 'aria-label': '清理', onClick: () => {} }
    const element = jsx('button', props, 'stable')
    expect(element.type).toBe('button')
    expect(element.key).toBe('stable')
    expect(element.props).toMatchObject({
      children: 'Cleanup',
      value: '清理',
      'aria-label': 'Cleanup',
      onClick: props.onClick,
    })
    expect(props.children).toBe('清理')
    expect(jsx('pre', { children: '清理' }).props).toMatchObject({ children: '清理' })
    expect(jsx('span', { children: '清理', 'data-i18n': 'off' }).props).toMatchObject({
      children: '清理',
    })
    expect(jsx('span', { children: '清理', className: 'mono' }).props).toMatchObject({
      children: '清理',
    })
  })
  it('translates adjacent text as a complete sentence', () => {
    setLanguagePreference('en')
    expect(jsxs('span', { children: ['总内存 ', '16 GB'] }).props).toMatchObject({
      children: ['Total memory 16 GB', null],
    })
  })
})

describe('searching localized display labels', () => {
  it('matches translated task kinds and maintenance descriptions without changing records', () => {
    const record: TaskRecord = {
      id: 'sample',
      kind: 'analyze',
      state: 'succeeded',
      path: '/example/清理',
      createdAt: 1,
      finishedAt: 2,
      message: null,
    }
    setLanguagePreference('en')
    expect(filterTasks([record], 'space analyzer', 'all')).toEqual([record])
    expect(maintenanceItems('thumbnail').length).toBeGreaterThan(0)
    setLanguagePreference('ja')
    expect(filterTasks([record], '容量分析', 'all')).toEqual([record])
    expect(maintenanceItems('サムネイル').length).toBeGreaterThan(0)
    expect(record.path).toBe('/example/清理')
    expect(record.kind).toBe('analyze')
  })
})
