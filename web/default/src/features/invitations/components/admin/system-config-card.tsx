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
import { useState } from 'react'
import { z } from 'zod'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Edit, Save, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { getSystemConfig, updateSystemConfig } from '../../api'
import { formatRebateAmount } from '../../lib/format'
import type { SystemConfig } from '../../types'

const formSchema = z.object({
  minRebateRequestAmount: z
    .number()
    .int()
    .min(0, 'Minimum rebate request amount must be at least 0'),
  rebateRequestFrequencyDays: z
    .number()
    .int()
    .min(1, 'Rebate request frequency must be at least 1 day'),
})

type FormData = z.infer<typeof formSchema>

export function SystemConfigCard() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [isEditing, setIsEditing] = useState(false)

  // 获取系统配置
  const { data: configData, isLoading } = useQuery({
    queryKey: ['systemConfig'],
    queryFn: async () => {
      const response = await getSystemConfig()
      return response.data
    },
  })

  const {
    register,
    handleSubmit,
    formState: { errors },
    reset,
  } = useForm<FormData>({
    resolver: zodResolver(formSchema),
    values: configData
      ? {
          minRebateRequestAmount: configData.minRebateRequestAmount,
          rebateRequestFrequencyDays: configData.rebateRequestFrequencyDays,
        }
      : undefined,
  })

  // 更新配置
  const updateMutation = useMutation({
    mutationFn: (data: SystemConfig) => updateSystemConfig(data),
    onSuccess: () => {
      toast.success(t('System configuration updated successfully'))
      queryClient.invalidateQueries({ queryKey: ['systemConfig'] })
      setIsEditing(false)
    },
    onError: (error: Error) => {
      toast.error(error.message || t('Failed to update system configuration'))
    },
  })

  const onSubmit = (data: FormData) => {
    updateMutation.mutate(data)
  }

  const handleCancel = () => {
    reset()
    setIsEditing(false)
  }

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>{t('System Configuration')}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className='text-muted-foreground'>{t('Loading...')}</div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card>
      <CardHeader className='flex flex-row items-center justify-between'>
        <CardTitle>{t('System Configuration')}</CardTitle>
        {!isEditing && (
          <Button
            variant='outline'
            size='sm'
            onClick={() => setIsEditing(true)}
          >
            <Edit className='mr-2 size-4' />
            {t('Edit')}
          </Button>
        )}
      </CardHeader>
      <CardContent>
        {isEditing ? (
          <form onSubmit={handleSubmit(onSubmit)} className='space-y-4'>
            <div className='grid gap-4 sm:grid-cols-2'>
              <div className='space-y-2'>
                <Label htmlFor='minRebateRequestAmount'>
                  {t('Minimum Rebate Request Amount')} ({t('cents')})
                </Label>
                <Input
                  id='minRebateRequestAmount'
                  type='number'
                  min='0'
                  {...register('minRebateRequestAmount', {
                    valueAsNumber: true,
                  })}
                />
                {errors.minRebateRequestAmount && (
                  <p className='text-destructive text-sm'>
                    {errors.minRebateRequestAmount.message}
                  </p>
                )}
                <p className='text-muted-foreground text-sm'>
                  {t('Amount in cents (100 = {{amount}})', {
                    amount: formatRebateAmount(100),
                  })}
                </p>
              </div>

              <div className='space-y-2'>
                <Label htmlFor='rebateRequestFrequencyDays'>
                  {t('Rebate Request Frequency (Days)')}
                </Label>
                <Input
                  id='rebateRequestFrequencyDays'
                  type='number'
                  min='1'
                  {...register('rebateRequestFrequencyDays', {
                    valueAsNumber: true,
                  })}
                />
                {errors.rebateRequestFrequencyDays && (
                  <p className='text-destructive text-sm'>
                    {errors.rebateRequestFrequencyDays.message}
                  </p>
                )}
                <p className='text-muted-foreground text-sm'>
                  {t('Minimum days between rebate requests')}
                </p>
              </div>
            </div>

            <div className='flex justify-end gap-2'>
              <Button
                type='button'
                variant='outline'
                onClick={handleCancel}
                disabled={updateMutation.isPending}
              >
                <X className='mr-2 size-4' />
                {t('Cancel')}
              </Button>
              <Button type='submit' disabled={updateMutation.isPending}>
                <Save className='mr-2 size-4' />
                {updateMutation.isPending ? t('Saving...') : t('Save')}
              </Button>
            </div>
          </form>
        ) : (
          <div className='grid gap-4 sm:grid-cols-2'>
            <div>
              <div className='text-muted-foreground text-sm'>
                {t('Minimum Rebate Request Amount')}
              </div>
              <div className='mt-1 text-lg font-medium'>
                {configData
                  ? formatRebateAmount(configData.minRebateRequestAmount)
                  : '-'}
              </div>
            </div>
            <div>
              <div className='text-muted-foreground text-sm'>
                {t('Rebate Request Frequency (Days)')}
              </div>
              <div className='mt-1 text-lg font-medium'>
                {configData
                  ? `${configData.rebateRequestFrequencyDays} ${t('days')}`
                  : '-'}
              </div>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
