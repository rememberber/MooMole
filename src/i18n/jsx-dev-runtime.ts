import { jsxDEV as reactJsxDEV } from 'react/jsx-dev-runtime'
import type { ElementType, Key } from 'react'
import { localizeProps } from './jsx-runtime'
export { Fragment } from 'react/jsx-runtime'
export type { JSX } from './jsx-runtime'
export function jsxDEV(
  type: ElementType,
  props: Record<string, unknown> | null,
  key: Key | undefined,
  isStaticChildren: boolean,
  source: { fileName: string; lineNumber: number; columnNumber: number },
  self: unknown,
) {
  return reactJsxDEV(type, localizeProps(type, props), key, isStaticChildren, source, self)
}
