import type { FileKind } from './large-files'

export interface DirectoryContext {
  query: string
  type: string
  sort: 'size' | 'name'
  page: number
  selectedPath: string | null
  scrollTop: number
}
export interface LargeFileContext {
  query: string
  kind: FileKind
  minimum: number
  sort: 'size' | 'name'
  page: number
  scrollTop: number
}
export interface AnalysisContext {
  view: 'directory' | 'large'
  directory: DirectoryContext
  large?: LargeFileContext
  windowScroll: number
}
export interface AnalysisExplorerHandle {
  capture: () => { path: string; context: AnalysisContext }
}
export interface LargeFileHandle {
  capture: () => LargeFileContext
}

// Session-only UI preferences. No result payload, size, state, permission or
// filesystem identity is retained. Repeated paths share their latest context.
export class AnalysisContexts {
  private entries = new Map<string, AnalysisContext>()

  remember(path: string, context: AnalysisContext) {
    this.entries.delete(path)
    this.entries.set(path, structuredClone(context))
    while (this.entries.size > 32) this.entries.delete(this.entries.keys().next().value!)
  }

  get(path: string): AnalysisContext | undefined {
    const value = this.entries.get(path)
    return value ? structuredClone(value) : undefined
  }

  retain(paths: string[]) {
    const retained = new Set(paths)
    for (const path of this.entries.keys()) if (!retained.has(path)) this.entries.delete(path)
  }
}
