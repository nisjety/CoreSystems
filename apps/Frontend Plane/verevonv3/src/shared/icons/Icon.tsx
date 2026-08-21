import { For, omit } from 'solid-js'
import type { JSX } from '@solidjs/web'

/**
 * lucide-solid has no Solid v2-compatible release (its compiled output
 * imports removed v1 APIs — mergeProps/splitProps from 'solid-js', and
 * 'solid-js/web' no longer exists as an export path at all in v2). This is a
 * local, v2-native re-implementation of just the rendering primitive
 * (lucide-solid's `Icon` component), so the icon set vendored in this
 * directory's index.tsx can render under Solid v2 with identical output.
 */

export type IconNode = ReadonlyArray<readonly [string, Record<string, string | number>]>

export type LucideProps = JSX.SvgSVGAttributes<SVGSVGElement> & {
  size?: string | number
  color?: string
  strokeWidth?: string | number
  absoluteStrokeWidth?: boolean
}

type InternalIconProps = LucideProps & { iconNode: IconNode; name: string }

const defaultAttributes = {
  xmlns: 'http://www.w3.org/2000/svg',
  width: 24,
  height: 24,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  'stroke-width': 2,
  'stroke-linecap': 'round' as const,
  'stroke-linejoin': 'round' as const,
}

function renderNode([tag, attrs]: readonly [string, Record<string, string | number>]): JSX.Element {
  switch (tag) {
    case 'path': return <path {...attrs} />
    case 'circle': return <circle {...attrs} />
    case 'rect': return <rect {...attrs} />
    case 'line': return <line {...attrs} />
    case 'polyline': return <polyline {...attrs} />
    case 'polygon': return <polygon {...attrs} />
    case 'ellipse': return <ellipse {...attrs} />
    default: return null
  }
}

export function Icon(allProps: InternalIconProps) {
  const rest = omit(allProps,
    'color', 'size', 'strokeWidth', 'children', 'class', 'name', 'iconNode', 'absoluteStrokeWidth',
  )

  const strokeWidth = () => {
    const raw = Number(allProps.strokeWidth ?? defaultAttributes['stroke-width'])
    if (!allProps.absoluteStrokeWidth) return raw
    return (raw * 24) / Number(allProps.size ?? defaultAttributes.width)
  }

  const className = () =>
    ['lucide', 'lucide-icon', `lucide-${allProps.name}`, allProps.class].filter(Boolean).join(' ')

  return (
    <svg
      {...defaultAttributes}
      {...rest}
      width={allProps.size ?? defaultAttributes.width}
      height={allProps.size ?? defaultAttributes.height}
      stroke={allProps.color ?? defaultAttributes.stroke}
      stroke-width={strokeWidth()}
      class={className()}
      aria-hidden={allProps.children ? undefined : 'true'}
    >
      <For each={allProps.iconNode}>{renderNode}</For>
      {allProps.children}
    </svg>
  )
}
