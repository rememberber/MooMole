export function ProblemRepairs({
  enabled,
  onPreview,
  onSearch,
}: {
  enabled: boolean
  onPreview: () => void
  onSearch: () => void
}) {
  return (
    <section className="problem-repairs" aria-label="按问题修复">
      <div className="problem-repair-row">
        <div>
          <strong>文件预览不显示</strong>
          <p className="muted">刷新当前用户的 Quick Look 缓存和预览服务，再打开原来的文件验证。</p>
        </div>
        <button className="button secondary compact" disabled={!enabled} onClick={onPreview}>
          修复预览…
        </button>
      </div>
      <div className="problem-repair-row">
        <div>
          <strong>文件搜索不到</strong>
          <p className="muted">选择个人文件夹提交 Spotlight 索引，再用文件名验证搜索结果。</p>
        </div>
        <button className="button secondary compact" disabled={!enabled} onClick={onSearch}>
          更新目录索引…
        </button>
      </div>
    </section>
  )
}
