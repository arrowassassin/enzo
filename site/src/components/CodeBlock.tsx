import { useCallback, useEffect, useRef, useState } from 'react'

interface CodeBlockProps {
  /** Shown in the bar; also the accessible language label. */
  lang: string
  code: string
}

/** Lines beginning with # are dimmed as comments — no highlighter, no payload. */
function render(code: string) {
  return code.split('\n').map((line, i) => {
    const key = `${i}-${line}`
    const isComment = line.trimStart().startsWith('#')
    return (
      <span key={key} className={isComment ? 'code__comment' : undefined}>
        {line}
        {'\n'}
      </span>
    )
  })
}

export function CodeBlock({ lang, code }: CodeBlockProps) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | undefined>(undefined)

  useEffect(() => () => window.clearTimeout(timer.current), [])

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(code)
    } catch {
      // Clipboard access can be refused (insecure context, denied permission);
      // fall back to selecting nothing rather than throwing at the user.
      return
    }
    setCopied(true)
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setCopied(false), 1600)
  }, [code])

  return (
    <div className="code">
      <div className="code__bar">
        <span className="code__lang">{lang}</span>
        <button
          type="button"
          className="code__copy"
          data-copied={copied}
          onClick={copy}
          aria-label={copied ? 'Copied to clipboard' : `Copy the ${lang} snippet`}
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre>
        <code>{render(code)}</code>
      </pre>
    </div>
  )
}
