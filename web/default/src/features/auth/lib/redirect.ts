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
export const DEFAULT_AUTH_REDIRECT = '/dashboard'

const AUTH_REDIRECT_STORAGE_KEY = 'auth:redirect'

export type AuthRedirectSearch = {
  redirect: string
}

export function getSafeAuthRedirectTarget(
  redirectTo?: string | null
): string | null {
  const target = redirectTo?.trim()
  if (!target) return null

  if (target.startsWith('/') && !target.startsWith('//')) {
    return target
  }

  if (typeof window === 'undefined') return null

  try {
    const url = new URL(target)
    if (url.origin !== window.location.origin) return null
    return `${url.pathname}${url.search}${url.hash}`
  } catch {
    return null
  }
}

export function getAuthRedirectSearch(
  redirectTo?: string | null
): AuthRedirectSearch | undefined {
  const target = getSafeAuthRedirectTarget(redirectTo)
  return target ? { redirect: target } : undefined
}

export function rememberAuthRedirect(redirectTo?: string | null): void {
  if (typeof window === 'undefined') return

  try {
    const target = getSafeAuthRedirectTarget(redirectTo)
    if (target) {
      window.sessionStorage.setItem(AUTH_REDIRECT_STORAGE_KEY, target)
    } else {
      window.sessionStorage.removeItem(AUTH_REDIRECT_STORAGE_KEY)
    }
  } catch {
    /* empty */
  }
}

export function takeRememberedAuthRedirect(): string | null {
  if (typeof window === 'undefined') return null

  try {
    const target = getSafeAuthRedirectTarget(
      window.sessionStorage.getItem(AUTH_REDIRECT_STORAGE_KEY)
    )
    window.sessionStorage.removeItem(AUTH_REDIRECT_STORAGE_KEY)
    return target
  } catch {
    return null
  }
}
