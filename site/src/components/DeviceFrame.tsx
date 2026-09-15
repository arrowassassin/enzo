import { shotUrl } from '../lib/asset'
import { screenFor } from '../lib/catalogue'

interface DeviceFrameProps {
  /** File name in public/shots, without the extension. */
  file: string
  size?: 'sm' | 'md' | 'lg'
  /** Drop the shell and show the panel behind a hairline instead. */
  plain?: boolean
  /** Small mono caption under the frame. */
  caption?: string
  priority?: boolean
}

/**
 * The screenshots are 1-bit black-on-white PNGs. In dark mode the frame turns
 * paper-coloured instead of the image being inverted, so the panel keeps
 * reading as printed ink on paper in both themes.
 */
export function DeviceFrame({
  file,
  size = 'md',
  plain = false,
  caption,
  priority = false,
}: DeviceFrameProps) {
  const screen = screenFor(file)
  const cls = [
    'device',
    size === 'lg' ? 'device--lg' : size === 'sm' ? 'device--sm' : '',
    plain ? 'device--plain' : '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <figure className={cls} style={{ margin: 0 }}>
      <div className="device__screen">
        <img
          src={shotUrl(file)}
          alt={screen.caption || screen.title}
          width={528}
          height={792}
          loading={priority ? 'eager' : 'lazy'}
          decoding={priority ? 'sync' : 'async'}
          {...(priority ? { fetchPriority: 'high' as const } : {})}
        />
      </div>
      {caption ? <figcaption className="device__caption">{caption}</figcaption> : null}
    </figure>
  )
}
