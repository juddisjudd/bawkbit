<script lang="ts">
  import type { Attachment } from 'svelte/attachments'

  let { map, label }: { map: number[]; label: string } = $props()

  function draw(values: number[]): Attachment<HTMLCanvasElement> {
    return (canvas) => {
      const paint = () => {
        const ratio = window.devicePixelRatio || 1
        const width = Math.max(1, Math.round(canvas.clientWidth * ratio))
        const height = Math.max(1, Math.round(canvas.clientHeight * ratio))
        canvas.width = width
        canvas.height = height
        const ctx = canvas.getContext('2d')
        if (!ctx) return
        const style = getComputedStyle(canvas)
        ctx.clearRect(0, 0, width, height)
        ctx.fillStyle = style.getPropertyValue('--track').trim() || '#2c2827'
        ctx.fillRect(0, 0, width, height)
        if (!values.length) return
        ctx.fillStyle = style.getPropertyValue('--fill').trim() || '#f6d56b'
        const step = width / values.length
        values.forEach((value, ix) => {
          if (!value) return
          ctx.globalAlpha = 0.25 + (0.75 * value) / 255
          ctx.fillRect(Math.floor(ix * step), 0, Math.ceil(step), height)
        })
        ctx.globalAlpha = 1
      }
      paint()
      const observer = new ResizeObserver(paint)
      observer.observe(canvas)
      const themes = new MutationObserver(paint)
      themes.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
      return () => {
        observer.disconnect()
        themes.disconnect()
      }
    }
  }
</script>

<div class="wrap" role="img" aria-label={label}><canvas class="map" aria-hidden="true" {@attach draw(map)}></canvas></div>

<style>
  .map {
    --track: var(--border-weak);
    --fill: var(--yolk);
    display: block;
    width: 100%;
    height: 12px;
    border-radius: 3px;
  }
</style>
