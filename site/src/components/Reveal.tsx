import type { ReactNode } from 'react'

interface RevealProps {
  children: ReactNode
  /** Milliseconds of stagger inside a row of siblings. */
  delay?: number
  className?: string
  as?: 'div' | 'li' | 'section'
}

/**
 * Marks a block for the page-level IntersectionObserver in `useReveal`.
 * Everything is visible without JavaScript-driven state changes as soon as the
 * observer fires once; reduced-motion users never see the transform at all.
 */
export function Reveal({ children, delay = 0, className, as = 'div' }: RevealProps) {
  const Tag = as
  const cls = className ? `reveal ${className}` : 'reveal'
  return (
    <Tag className={cls} style={delay ? { transitionDelay: `${delay}ms` } : undefined}>
      {children}
    </Tag>
  )
}
