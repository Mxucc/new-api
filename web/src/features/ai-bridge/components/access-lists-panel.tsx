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
import { Ban, ListChecks, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ConfirmDialog } from '@/components/confirm-dialog'
import { StaticDataTable } from '@/components/data-table'
import { TruncatedText } from '@/components/truncated-text'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { TitledCard } from '@/components/ui/titled-card'

import {
  useCreateAiBridgeListEntry,
  useDeleteAiBridgeListEntry,
} from '../hooks/use-ai-bridge-console'
import { isIpAddress } from '../lib/format'
import type { AiBridgeListKind, AiBridgeSnapshot } from '../types'
import { TruncatedNotice } from './truncated-notice'

interface AccessListRow {
  id: string
  primary: string
  fingerprint?: string
}

interface ListSectionProps {
  kind: AiBridgeListKind
  title: string
  description: string
  icon: typeof Ban
  rows: AccessListRow[]
  total: number
  truncated: boolean
  csrfToken: string
  requireIpAddress: boolean
}

function ListSection(props: ListSectionProps) {
  const { t } = useTranslation()
  const createEntry = useCreateAiBridgeListEntry()
  const deleteEntry = useDeleteAiBridgeListEntry()
  const [value, setValue] = useState('')
  const [errorKey, setErrorKey] = useState<string | null>(null)
  const [pending, setPending] = useState<AccessListRow | null>(null)
  const withFingerprint = props.rows.some((row) => row.fingerprint != null)

  const handleSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const trimmed = value.trim()
    if (!trimmed) {
      setErrorKey('Enter a value first.')
      return
    }
    if (props.requireIpAddress && !isIpAddress(trimmed)) {
      setErrorKey('Enter a valid IPv4 or IPv6 address.')
      return
    }
    setErrorKey(null)
    createEntry.mutate(
      { csrfToken: props.csrfToken, kind: props.kind, value: trimmed },
      { onSuccess: () => setValue('') }
    )
  }

  return (
    <TitledCard
      title={props.title}
      description={props.description}
      icon={<props.icon />}
      iconTone='chart-2'
    >
      <div className='space-y-4'>
        <form
          className='flex flex-wrap items-start gap-2'
          onSubmit={handleSubmit}
        >
          <Input
            className='max-w-md min-w-0 flex-1'
            value={value}
            aria-label={t('Add to {{list}}', { list: props.title })}
            aria-invalid={errorKey != null}
            placeholder={
              props.requireIpAddress
                ? t('IPv4 or IPv6 address')
                : t('API Key or its SHA-256 identifier')
            }
            onChange={(event) => setValue(event.target.value)}
          />
          <Button type='submit' disabled={createEntry.isPending}>
            {t('Add')}
          </Button>
        </form>
        {errorKey != null && (
          <p className='text-destructive text-xs' role='alert'>
            {t(errorKey)}
          </p>
        )}

        <p className='text-muted-foreground text-xs'>
          {t('{{total}} entries in this list', { total: props.total })}
        </p>
        {props.truncated && (
          <TruncatedNotice shown={props.rows.length} total={props.total} />
        )}

        <StaticDataTable
          columns={[
            {
              id: 'entry',
              header: props.requireIpAddress ? t('IP address') : t('Entry'),
              cell: (row: AccessListRow) => (
                <TruncatedText
                  text={row.primary}
                  className={props.requireIpAddress ? 'font-mono' : undefined}
                />
              ),
            },
            ...(withFingerprint
              ? [
                  {
                    id: 'fingerprint',
                    header: t('Fingerprint'),
                    cell: (row: AccessListRow) =>
                      row.fingerprint ? (
                        <TruncatedText
                          text={row.fingerprint}
                          className='font-mono'
                        />
                      ) : (
                        <span className='text-muted-foreground'>-</span>
                      ),
                  },
                ]
              : []),
            {
              id: 'actions',
              header: t('Actions'),
              className: 'text-right',
              cellClassName: 'text-right',
              cell: (row: AccessListRow) => (
                <Button
                  variant='outline'
                  size='sm'
                  onClick={() => setPending(row)}
                >
                  {t('Remove')}
                </Button>
              ),
            },
          ]}
          data={props.rows}
          getRowKey={(row) => row.id}
          emptyContent={t('This list is empty')}
        />
      </div>

      <ConfirmDialog
        open={pending != null}
        onOpenChange={(open) => {
          if (!open) setPending(null)
        }}
        title={t('Remove this entry?')}
        desc={t(
          'The entry is removed from the policy list and the file is rewritten.'
        )}
        confirmText={t('Remove')}
        destructive
        isLoading={deleteEntry.isPending}
        handleConfirm={() => {
          if (!pending) return
          deleteEntry.mutate(
            { csrfToken: props.csrfToken, kind: props.kind, id: pending.id },
            { onSettled: () => setPending(null) }
          )
        }}
      />
    </TitledCard>
  )
}

export function AccessListsPanel(props: {
  snapshot: AiBridgeSnapshot
  csrfToken: string
}) {
  const { t } = useTranslation()
  const blacklist = props.snapshot.blacklist
  const whitelist = props.snapshot.whitelist
  const keyRows = (entries: AiBridgeSnapshot['blacklist']['api_keys']) =>
    entries.map((entry) => ({
      id: entry.id,
      primary: entry.masked,
      fingerprint: entry.fingerprint,
    }))

  return (
    <div className='space-y-6'>
      <ListSection
        kind='blacklist_key'
        title={t('API Key blacklist')}
        description={t('Requests using these API Keys are rejected.')}
        icon={Ban}
        rows={keyRows(blacklist.api_keys)}
        total={blacklist.total_api_keys}
        truncated={blacklist.api_keys_truncated}
        csrfToken={props.csrfToken}
        requireIpAddress={false}
      />
      <ListSection
        kind='blacklist_ip'
        title={t('IP blacklist')}
        description={t('Requests from these client IP addresses are rejected.')}
        icon={ListChecks}
        rows={blacklist.client_ips.map((ip) => ({ id: ip, primary: ip }))}
        total={blacklist.total_client_ips}
        truncated={blacklist.client_ips_truncated}
        csrfToken={props.csrfToken}
        requireIpAddress
      />
      <ListSection
        kind='whitelist_key'
        title={t('API Key whitelist')}
        description={t('These API Keys are exempt from the Cyber Policy ban.')}
        icon={ShieldCheck}
        rows={keyRows(whitelist.api_keys)}
        total={whitelist.total_api_keys}
        truncated={whitelist.api_keys_truncated}
        csrfToken={props.csrfToken}
        requireIpAddress={false}
      />
    </div>
  )
}
