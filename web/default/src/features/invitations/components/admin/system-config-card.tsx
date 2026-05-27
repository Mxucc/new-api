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
import { useTranslation } from 'react-i18next'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Edit, Save, X } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { SystemConfig } from '../../types'
import { getSystemConfig, updateSystemConfig } from '../../api'

const formSchema = z.object({
  min_withdrawal_amount: z
    .number()
    .int()
    .min(0, 'Minimum withdrawal amount must be at least 0'),
  withdrawal_frequency_days: z
    .number()
    .int()
    .min(1, 'Withdrawal frequency must be at least 1 day'),
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
          min_withdrawal_amount: configData.min_withdrawal_amount,
          withdrawal_frequency_days: configData.withdrawal_frequency_days,
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

  const formatAmount = (amount: number) => {
    return `¥${(amount / 100).toFixed(2)}`
  }

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>{t('System Configuration')}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="text-muted-foreground">{t('Loading...')}</div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle>{t('System Configuration')}</CardTitle>
        {!isEditing && (
          <Button variant="outline" size="sm" onClick={() => setIsEditing(true)}>
            <Edit className="mr-2 size-4" />
            {t('Edit')}
          </Button>
        )}
      </CardHeader>
      <CardContent>
        {isEditing ? (
          <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="min_withdrawal_amount">
                  {t('Minimum Withdrawal Amount')} ({t('cents')})
                </Label>
                <Input
                  id="min_withdrawal_amount"
                  type="number"
                  min="0"
                  {...register('min_withdrawal_amount', { valueAsNumber: true })}
                />
                {errors.min_withdrawal_amount && (
                  <p className="text-sm text-destructive">
                    {errors.min_withdrawal_amount.message}
                  </p>
                )}
                <p className="text-sm text-muted-foreground">
                  {t('Amount in cents (100 = ¥1.00)')}
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="withdrawal_frequency_days">
                  {t('Withdrawal Frequency (Days)')}
                </Label>
                <Input
                  id="withdrawal_frequency_days"
                  type="number"
                  min="1"
                  {...register('withdrawal_frequency_days', { valueAsNumber: true })}
                />
                {errors.withdrawal_frequency_days && (
                  <p className="text-sm text-destructive">
                    {errors.withdrawal_frequency_days.message}
                  </p>
                )}
                <p className="text-sm text-muted-foreground">
                  {t('Minimum days between withdrawal requests')}
                </p>
              </div>
            </div>

            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={handleCancel}
                disabled={updateMutation.isPending}
              >
                <X className="mr-2 size-4" />
                {t('Cancel')}
              </Button>
              <Button type="submit" disabled={updateMutation.isPending}>
                <Save className="mr-2 size-4" />
                {updateMutation.isPending ? t('Saving...') : t('Save')}
              </Button>
            </div>
          </form>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2">
            <div>
              <div className="text-sm text-muted-foreground">
                {t('Minimum Withdrawal Amount')}
              </div>
              <div className="mt-1 text-lg font-medium">
                {configData ? formatAmount(configData.min_withdrawal_amount) : '-'}
              </div>
            </div>
            <div>
              <div className="text-sm text-muted-foreground">
                {t('Withdrawal Frequency (Days)')}
              </div>
              <div className="mt-1 text-lg font-medium">
                {configData ? `${configData.withdrawal_frequency_days} ${t('days')}` : '-'}
              </div>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
