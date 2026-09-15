import type { ReactNode } from 'react'

interface CalloutProps {
  title: string
  tone?: 'note' | 'warn'
  children: ReactNode
}

export function Callout({ title, tone = 'note', children }: CalloutProps) {
  return (
    <aside className={tone === 'warn' ? 'callout callout--warn' : 'callout'}>
      <p className="callout__title">{title}</p>
      {children}
    </aside>
  )
}
