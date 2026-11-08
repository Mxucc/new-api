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
import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useId } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Dialog } from '@/components/dialog'
import { Button } from '@/components/ui/button'
import { Field, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'

import {
  CONNECT_CLIENTS_QUERY_KEY,
  createConnectClient,
  updateConnectClient,
} from '../api'
import {
  CONNECT_APP_FORM_DEFAULT_VALUES,
  createConnectAppFormSchema,
  toConnectAppFormValues,
  toConnectClientPayload,
  type ConnectAppFormValues,
} from '../lib/app-form'
import { getConnectErrorMessage } from '../lib/error'
import type { ConnectClient, CreatedConnectClient } from '../types'

interface ConnectAppFormDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** `null` registers a new application; a client edits that application. */
  client: ConnectClient | null
  onCreated: (created: CreatedConnectClient) => void
}

export function ConnectAppFormDialog(props: ConnectAppFormDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const formId = useId()
  const isEdit = props.client !== null

  const form = useForm<ConnectAppFormValues>({
    resolver: zodResolver(createConnectAppFormSchema(t)),
    defaultValues: CONNECT_APP_FORM_DEFAULT_VALUES,
  })
  const errors = form.formState.errors

  const mutation = useMutation({
    mutationFn: async (values: ConnectAppFormValues) => {
      const payload = toConnectClientPayload(values)
      if (props.client) {
        await updateConnectClient(props.client.id, payload)
        return null
      }
      return createConnectClient(payload)
    },
    onSuccess: (created) => {
      queryClient.invalidateQueries({ queryKey: CONNECT_CLIENTS_QUERY_KEY })
      form.reset(CONNECT_APP_FORM_DEFAULT_VALUES)
      props.onOpenChange(false)
      if (created) {
        toast.success(t('Application created'))
        props.onCreated(created)
        return
      }
      toast.success(t('Application updated'))
    },
    onError: (error: unknown) => {
      toast.error(
        getConnectErrorMessage(
          error,
          isEdit
            ? t('Failed to update the application')
            : t('Failed to create the application')
        )
      )
    },
  })

  useEffect(() => {
    if (props.open) {
      form.reset(toConnectAppFormValues(props.client))
    }
  }, [props.open, props.client, form])

  const title = isEdit ? t('Edit application') : t('New application')
  const description = isEdit
    ? t('Update how this application appears and where it redirects.')
    : t('Register an application to sign users in with Connect OAuth2.')
  let submitLabel = isEdit ? t('Save') : t('Create application')
  if (mutation.isPending) submitLabel = t('Saving...')

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={title}
      description={description}
      footer={
        <>
          <Button
            type='button'
            variant='outline'
            disabled={mutation.isPending}
            onClick={() => props.onOpenChange(false)}
          >
            {t('Cancel')}
          </Button>
          <Button type='submit' form={formId} disabled={mutation.isPending}>
            {submitLabel}
          </Button>
        </>
      }
    >
      <form
        id={formId}
        className='space-y-4'
        onSubmit={form.handleSubmit((values) => mutation.mutate(values))}
      >
        <Field>
          <FieldLabel htmlFor={`${formId}-name`}>
            {t('Application name')}
          </FieldLabel>
          <Input
            id={`${formId}-name`}
            placeholder={t('Your application name')}
            aria-invalid={errors.name != null}
            {...form.register('name')}
          />
          <FieldError errors={[errors.name]} />
        </Field>

        <Field>
          <FieldLabel htmlFor={`${formId}-homepage`}>
            {t('Application homepage')}
          </FieldLabel>
          <Input
            id={`${formId}-homepage`}
            placeholder='https://example.com'
            aria-invalid={errors.homepage_url != null}
            {...form.register('homepage_url')}
          />
          <FieldError errors={[errors.homepage_url]} />
        </Field>

        <Field>
          <FieldLabel htmlFor={`${formId}-logo`}>
            {t('Application logo URL')}
          </FieldLabel>
          <Input
            id={`${formId}-logo`}
            placeholder='https://example.com/logo.png'
            aria-invalid={errors.logo_url != null}
            {...form.register('logo_url')}
          />
          <FieldError errors={[errors.logo_url]} />
        </Field>

        <Field>
          <FieldLabel htmlFor={`${formId}-description`}>
            {t('Application description')}
          </FieldLabel>
          <Textarea
            id={`${formId}-description`}
            rows={3}
            placeholder={t('Briefly describe your application')}
            aria-invalid={errors.description != null}
            {...form.register('description')}
          />
          <FieldError errors={[errors.description]} />
        </Field>

        <Field>
          <FieldLabel htmlFor={`${formId}-redirect`}>
            {t('Redirect URI')}
          </FieldLabel>
          <Input
            id={`${formId}-redirect`}
            placeholder='https://example.com/callback'
            aria-invalid={errors.redirect_uri != null}
            {...form.register('redirect_uri')}
          />
          <p className='text-muted-foreground text-xs'>
            {t('The address the authorization code is sent to.')}
          </p>
          <FieldError errors={[errors.redirect_uri]} />
        </Field>
      </form>
    </Dialog>
  )
}
