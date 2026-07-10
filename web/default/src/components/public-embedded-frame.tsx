/*
Copyright (C) 2023-2026 QuantumNous

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as
published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.

For commercial licensing, please contact support@quantumnous.com
*/
import { useCallback, useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { useTheme } from '@/context/theme-provider'
import { cn } from '@/lib/utils'

type PublicEmbeddedFrameProps = {
  src: string
  title: string
  className?: string
}

function normalizeEmbeddedTheme(theme: string) {
  return theme === 'system' ? 'auto' : theme
}

function getParentOrigin() {
  if (typeof window === 'undefined') return ''
  return window.location.origin
}

function buildEmbeddedSrc(src: string, themeMode: string, language: string) {
  try {
    const url = new URL(src)
    const parentOrigin = getParentOrigin()

    url.searchParams.set('embed', '1')
    url.searchParams.set('theme', themeMode)
    url.searchParams.set('lang', language)

    if (parentOrigin) {
      url.searchParams.set('parent_origin', parentOrigin)
      url.searchParams.set('web_url', parentOrigin)
    }

    return url.toString()
  } catch {
    return src
  }
}

export function PublicEmbeddedFrame(props: PublicEmbeddedFrameProps) {
  const frameRef = useRef<HTMLIFrameElement | null>(null)
  const { theme, resolvedTheme } = useTheme()
  const { i18n } = useTranslation()
  const language = i18n.resolvedLanguage || i18n.language || 'en'
  const themeMode = normalizeEmbeddedTheme(theme)

  const src = useMemo(
    () => buildEmbeddedSrc(props.src, themeMode, language),
    [props.src, themeMode, language]
  )

  const postUiState = useCallback(() => {
    const frameWindow = frameRef.current?.contentWindow
    if (!frameWindow) return

    const payload = {
      type: 'new-api:ui-state',
      themeMode,
      theme: themeMode,
      mode: themeMode,
      resolvedTheme,
      dark: resolvedTheme === 'dark',
      lang: language,
      locale: language,
      currentLang: language,
      currentLanguage: language,
      data: {
        themeMode,
        resolvedTheme,
        lang: language,
        locale: language,
      },
    }

    frameWindow.postMessage(payload, '*')
  }, [language, resolvedTheme, themeMode])

  useEffect(() => {
    postUiState()
  }, [postUiState, src])

  return (
    <iframe
      ref={frameRef}
      src={src}
      className={cn('w-full border-none', props.className)}
      title={props.title}
      allow='clipboard-read; clipboard-write'
      sandbox='allow-forms allow-popups allow-popups-to-escape-sandbox allow-scripts'
      loading='eager'
      onLoad={postUiState}
    />
  )
}
