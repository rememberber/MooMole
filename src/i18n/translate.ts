import messages from './messages.json' with { type: 'json' }
import { getLanguage } from './locale'

const catalog = new Map<string, readonly [string, string]>(
  (messages as [string, string, string][]).map(([source, en, ja]) => [
    source.replace(/\s+/g, ' ').trim(),
    [en.trim(), ja.trim()],
  ]),
)
const normalize = (text: string) => text.replace(/\s+/g, ' ').trim()
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
// Captures identifying user objects are literal data, even if a filename is
// identical to a UI label (for example a folder named “清理”). Other captures
// may be trusted display labels such as task kinds or result states.
const literalValues = new Map<string, readonly number[]>([
  ...[
    '保护 {0}',
    '移除保护 {0}',
    '审阅 {0}',
    '选择 {0}',
    '对比 {0}',
    '查看 {0}',
    '查看 {0} 详情',
    '在 Finder 中显示 {0}',
    '在 Finder 中显示大文件 {0}',
    '分析 {0} 所在目录',
    '分析目录 {0}',
    '返回：{0}',
    '前进：{0}',
    '上一级：{0}',
    '请求目录：{0}',
    '记录路径：{0}',
    '仓库：{0}',
    '项目线索 {0}',
  ].map((source): [string, readonly number[]] => [source, [0]]),
  ['按 Mole 名称与产品规则，为「{0}」查询 {1} 个位置，仅供审阅，本地显示名称和归属未认证。', [0]],
  ['按 Mole 渠道名称规则补查「{0}」；可能与其他版本或应用共用，全部保留。', [0]],
  ['按 Mole 包名与清单名称边界补查「{0}」；仅为派生线索，可能共用，全部保留。', [0]],
])
const templates = [...catalog]
  .filter(([source]) => /\{\d+\}/.test(source))
  .map(([source, translations]) => {
    const keys: string[] = []
    const pattern = normalize(source)
      .split(/(\{\d+\})/)
      .map((part) => {
        if (/^\{\d+\}$/.test(part)) {
          keys.push(part)
          return '(.*?)'
        }
        return escape(part)
      })
      .join('')
    return {
      pattern: new RegExp(`^${pattern}$`, 's'),
      keys,
      translations,
      literal: literalValues.get(source) ?? [],
    }
  })
  .sort((a, b) => b.pattern.source.length - a.pattern.source.length)

// This is a presentation boundary only. Structured plans, paths, selections,
// protocol validation and original diagnostic output remain in their source form.
export function translate(source: string): string {
  const language = getLanguage()
  if (language === 'zh' || !/[\p{Script=Han}]/u.test(source)) return source
  const text = normalize(source)
  const index = language === 'en' ? 0 : 1
  const exact = catalog.get(source) ?? catalog.get(text)
  let result = exact?.[index]
  if (result === undefined) {
    for (const template of templates) {
      const match = source.trim().match(template.pattern)
      if (!match) continue
      const values = new Map(template.keys.map((key, i) => [key, match[i + 1]]))
      result = template.translations[index].replace(/\{\d+\}/g, (key) => {
        const value = values.get(key) ?? key
        return template.literal.includes(Number(key.slice(1, -1))) ? value : translate(value)
      })
      break
    }
  }
  if (result === undefined) return source
  const leading = source.match(/^\s*/)?.[0] ?? ''
  const trailing = source.match(/\s*$/)?.[0] ?? ''
  return `${leading}${result}${trailing}`
}
