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
import { useEffect } from 'react'
import { createFileRoute, useNavigate, useSearch } from '@tanstack/react-router'
import i18next from 'i18next'
import { toast } from 'sonner'
import { z } from 'zod'
import { useAuthStore, type AuthUser } from '@/stores/auth-store'
import { getSelf } from '@/lib/api'
import { wechatLoginByCode } from '@/features/auth/api'
import {
  DEFAULT_AUTH_REDIRECT,
  getAuthRedirectSearch,
  getSafeAuthRedirectTarget,
} from '@/features/auth/lib/redirect'

const searchSchema = z.object({
  redirect: z.string().optional(),
  provider: z
    .enum(['github', 'discord', 'oidc', 'linuxdo', 'telegram', 'wechat'])
    .optional(),
  code: z.string().optional(),
  state: z.string().optional(),
})

function OAuthComponent() {
  const navigate = useNavigate()
  const search = useSearch({ from: '/(auth)/oauth' })

  useEffect(() => {
    ;(async () => {
      try {
        const redirectTarget = getSafeAuthRedirectTarget(search?.redirect)

        if (search?.provider === 'wechat' && search.code) {
          await wechatLoginByCode(search.code)
        }
        const res = await getSelf()
        if (res?.success) {
          useAuthStore.getState().auth.setUser(res.data as AuthUser)
          if (redirectTarget) {
            navigate({
              href: redirectTarget,
              replace: true,
              reloadDocument: true,
            })
          } else {
            navigate({ to: DEFAULT_AUTH_REDIRECT, replace: true })
          }
          return
        }
      } catch {
        /* empty */
      }
      toast.error(i18next.t('OAuth failed'))
      navigate({
        to: '/sign-in',
        search: getAuthRedirectSearch(search?.redirect),
        replace: true,
      })
    })()
  }, [navigate, search])

  return null
}

export const Route = createFileRoute('/(auth)/oauth')({
  validateSearch: searchSchema,
  component: OAuthComponent,
})
