import { jsx as reactJsx, jsxs as reactJsxs, Fragment } from 'react/jsx-runtime'
import type { JSX as ReactJSX, ReactNode, ElementType, Key } from 'react'
import { translate } from './translate'

export { Fragment }
export namespace JSX {
  export type ElementType = ReactJSX.ElementType
  export type Element = ReactJSX.Element
  export type ElementClass = ReactJSX.ElementClass
  export type ElementAttributesProperty = ReactJSX.ElementAttributesProperty
  export type ElementChildrenAttribute = ReactJSX.ElementChildrenAttribute
  export type LibraryManagedAttributes<C, P> = ReactJSX.LibraryManagedAttributes<C, P>
  export type IntrinsicAttributes = ReactJSX.IntrinsicAttributes
  export type IntrinsicClassAttributes<T> = ReactJSX.IntrinsicClassAttributes<T>
  export type IntrinsicElements = ReactJSX.IntrinsicElements
}
function localizeChildren(children: ReactNode): ReactNode {
  if (typeof children === 'string') return translate(children)
  if (!Array.isArray(children)) return children
  // React emits adjacent text and expression values as separate children.
  // Translate their complete sentence so languages can reorder placeholders.
  const output: ReactNode[] = []
  let run: (string | number)[] = []
  const flush = () => {
    if (!run.length) return
    const source = run.join('')
    const translated = translate(source)
    if (translated !== source) output.push(translated, ...Array(run.length - 1).fill(null))
    else output.push(...run.map((item) => (typeof item === 'string' ? translate(item) : item)))
    run = []
  }
  for (const child of children) {
    if (typeof child === 'string' || typeof child === 'number') run.push(child)
    else {
      flush()
      output.push(Array.isArray(child) ? localizeChildren(child) : child)
    }
  }
  flush()
  return output
}
export function localizeProps(type: ElementType, props: Record<string, unknown> | null) {
  if (typeof type !== 'string' || !props || props['data-i18n'] === 'off') return props
  // Logs and monospace fields are original evidence or user-controlled names /
  // paths. Do not reinterpret their content as interface copy.
  const raw =
    type === 'pre' ||
    type === 'code' ||
    /\b(mono|[\w-]*path[\w-]*)\b/.test(String(props.className ?? ''))
  const next = { ...props }
  if (!raw && 'children' in next) next.children = localizeChildren(next.children as ReactNode)
  for (const key of ['aria-label', 'aria-description', 'placeholder', 'title', 'alt'])
    if (typeof next[key] === 'string') next[key] = translate(next[key])
  return next
}
// Keep the original React element type, key and ref; switching language never
// remounts a page, cancels a task or replaces an input with a wrapper component.
export const jsx = (type: ElementType, props: Record<string, unknown> | null, key?: Key) =>
  reactJsx(type, localizeProps(type, props), key)
export const jsxs = (type: ElementType, props: Record<string, unknown> | null, key?: Key) =>
  reactJsxs(type, localizeProps(type, props), key)
