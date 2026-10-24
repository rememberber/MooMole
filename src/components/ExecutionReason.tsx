// Presentation only. This never determines eligibility or changes saved facts.
export function ExecutionReason({ reason }: { reason: string }) {
  const marker = ' 诊断：视图状态 '
  const at = reason.indexOf(marker)
  if (at <= 0) return <p>{reason}</p>
  return (
    <>
      <p>{reason.slice(0, at)}</p>
      <details className="execution-diagnostic">
        <summary>检查诊断</summary>
        <p>{reason.slice(at + 1)}</p>
      </details>
    </>
  )
}
