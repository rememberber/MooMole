import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { parseAst } from 'rolldown/parseAst'

const han = /\p{Script=Han}/u
const normalize = (value) => value.replace(/\s+/g, ' ').trim()
// Match React's JSX whitespace folding, including indentation-only lines.
function jsxText(text) {
  const lines = text.split(/\r\n|\n|\r/)
  const last = lines.findLastIndex((line) => /[^ \t]/.test(line))
  return lines
    .map((line, i) => {
      let value = line.replace(/\t/g, ' ')
      if (i !== 0) value = value.replace(/^ +/, '')
      if (i !== lines.length - 1) value = value.replace(/ +$/, '')
      return value && i < last ? `${value} ` : value
    })
    .join('')
}
export function sourceMessages() {
  const messages = new Set()
  function add(value) {
    if (han.test(value)) messages.add(normalize(value))
  }
  function visit(node) {
    if (!node || typeof node !== 'object') return
    if (node.type === 'Literal' && typeof node.value === 'string') add(node.value)
    if (node.type === 'JSXText') add(jsxText(node.value))
    if (node.type === 'TemplateLiteral')
      add(
        node.quasis
          .map((q, i) => q.value.cooked + (i < node.expressions.length ? `{${i}}` : ''))
          .join(''),
      )
    if (node.type === 'JSXElement' || node.type === 'JSXFragment') {
      let text = '',
        count = 0
      const flush = () => {
        if (count) add(text)
        text = ''
        count = 0
      }
      for (const child of node.children) {
        if (child.type === 'JSXText') text += jsxText(child.value)
        else if (child.type === 'JSXExpressionContainer') {
          if (child.expression.type === 'Literal' && typeof child.expression.value === 'string')
            text += child.expression.value
          else if (child.expression.type !== 'JSXEmptyExpression') text += `{${count++}}`
        } else flush()
      }
      flush()
    }
    for (const value of Object.values(node))
      if (Array.isArray(value)) value.forEach(visit)
      else if (value && typeof value === 'object') visit(value)
  }
  function walk(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = join(directory, entry.name)
      if (entry.isDirectory()) {
        if (entry.name !== 'i18n') walk(file)
        continue
      }
      if (/\.test\./.test(file)) continue
      if (/\.tsx?$/.test(file))
        visit(
          parseAst(readFileSync(file, 'utf8'), { lang: file.endsWith('tsx') ? 'tsx' : 'ts' }, file),
        )
      else if (file.endsWith('.json')) {
        const values = (v) => {
          if (typeof v === 'string') add(v)
          else if (v && typeof v === 'object') Object.values(v).forEach(values)
        }
        values(JSON.parse(readFileSync(file, 'utf8')))
      }
    }
  }
  walk('src')
  return [...messages].filter((source) => !['简体中文', '日本語'].includes(source))
}
if (process.argv[1]?.endsWith('i18n-catalog.mjs')) {
  const catalog = JSON.parse(readFileSync('src/i18n/messages.json', 'utf8'))
  const sources = new Set(catalog.map(([source]) => normalize(source)))
  const missing = sourceMessages().filter((source) => !sources.has(source))
  if (missing.length) {
    process.stderr.write(`Missing ${missing.length} source translations:\n${missing.join('\n')}\n`)
    process.exitCode = 1
  } else console.log(`Verified ${catalog.length} Chinese / English / Japanese messages.`)
}
