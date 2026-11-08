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
import { z } from 'zod'

import type { ConnectClient, ConnectClientPayload } from '../types'

/**
 * Application form shared by the create and edit dialogs. Messages are
 * translated by the caller so validation stays localized.
 */
export const createConnectAppFormSchema = (t: (key: string) => string) =>
  z.object({
    name: z.string().trim().min(1, t('Please enter the application name')),
    homepage_url: z
      .string()
      .trim()
      .min(1, t('Please enter the application homepage'))
      .url(t('Please enter a valid URL')),
    logo_url: z.string().trim().optional().or(z.literal('')),
    description: z.string().trim().optional().or(z.literal('')),
    redirect_uri: z
      .string()
      .trim()
      .min(1, t('Please enter the redirect URI'))
      .url(t('Please enter a valid URL')),
  })

export type ConnectAppFormValues = z.infer<
  ReturnType<typeof createConnectAppFormSchema>
>

export const CONNECT_APP_FORM_DEFAULT_VALUES: ConnectAppFormValues = {
  name: '',
  homepage_url: '',
  logo_url: '',
  description: '',
  redirect_uri: '',
}

export function toConnectAppFormValues(
  client?: ConnectClient | null
): ConnectAppFormValues {
  if (!client) return CONNECT_APP_FORM_DEFAULT_VALUES
  return {
    name: client.name,
    homepage_url: client.homepage_url,
    logo_url: client.logo_url,
    description: client.description,
    redirect_uri: client.redirect_uri,
  }
}

/** Empty optional fields are sent as empty strings so an edit can clear them. */
export function toConnectClientPayload(
  values: ConnectAppFormValues
): ConnectClientPayload {
  return {
    name: values.name.trim(),
    homepage_url: values.homepage_url.trim(),
    description: (values.description ?? '').trim(),
    logo_url: (values.logo_url ?? '').trim(),
    redirect_uri: values.redirect_uri.trim(),
  }
}
