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
import { useTranslation } from 'react-i18next'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import type { WithdrawalRequestAdmin } from '../../types'
import { approveWithdrawal, rejectWithdrawal, completeWithdrawal } from '../../api'

const approveSchema = z.object({
  note: z.string().optional(),
})

const rejectSchema = z.object({
  reason: z.string().min(1, 'Reject reason is required'),
  note: z.string().optional(),
})

type ApproveFormData = z.infer<typeof approveSchema>
type RejectFormData = z.infer<typeof rejectSchema>

interface WithdrawalActionDialogProps {
  open: boolean
  onClose: () => void
  request: WithdrawalRequestAdmin | null
  actionType: 'approve' | 'reject' | 'complete'
}

export function WithdrawalActionDialog({
  open,
  onClose,
  request,
  actionType,
}: WithdrawalActionDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const approveForm = useForm<ApproveFormData>({
    resolver: zodResolver(approveSchema),
    defaultValues: { note: '' },
  })

  const rejectForm = useForm<RejectFormData>({
    resolver: zodResolver(rejectSchema),
    defaultValues: { reason: '', note: '' },
  })

  // 通过申请
  const approveMutation = useMutation({
    mutationFn: ({ id, data }: { id: number; data?: ApproveFormData }) =>
      approveWithdrawal(id, data),
    onSuccess: () => {
      toast.success(t('Withdrawal request approved'))
      queryClient.invalidateQueries({ queryKey: ['adminWithdrawalRequests'] })
      onClose()
      approveForm.reset()
    },
    onError: (error: Error) => {
      toast.error(error.message || t('Failed to approve withdrawal request'))
    },
  })

  // 拒绝申请
  const rejectMutation = useMutation({
    mutationFn: ({ id, data }: { id: number; data: RejectFormData }) =>
      rejectWithdrawal(id, data),
    onSuccess: () => {
      toast.success(t('Withdrawal request rejected'))
      queryClient.invalidateQueries({ queryKey: ['adminWithdrawalRequests'] })
      onClose()
      rejectForm.reset()
    },
    onError: (error: Error) => {
      toast.error(error.message || t('Failed to reject withdrawal request'))
    },
  })

  // 标记完成
  const completeMutation = useMutation({
    mutationFn: (id: number) => completeWithdrawal(id),
    onSuccess: () => {
      toast.success(t('Withdrawal marked as completed'))
      queryClient.invalidateQueries({ queryKey: ['adminWithdrawalRequests'] })
      onClose()
    },
    onError: (error: Error) => {
      toast.error(error.message || t('Failed to complete withdrawal'))
    },
  })

  const handleApprove = (data: ApproveFormData) => {
    if (!request) return
    approveMutation.mutate({ id: request.id, data })
  }

  const handleReject = (data: RejectFormData) => {
    if (!request) return
    rejectMutation.mutate({ id: request.id, data })
  }

  const handleComplete = () => {
    if (!request) return
    if (window.confirm(t('Are you sure you want to mark this withdrawal as completed?'))) {
      completeMutation.mutate(request.id)
    }
  }

  const formatAmount = (amount: number) => {
    return `¥${(amount / 100).toFixed(2)}`
  }

  const isPending =
    approveMutation.isPending || rejectMutation.isPending || completeMutation.isPending

  if (!request) return null

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>
            {actionType === 'approve' && t('Approve Withdrawal')}
            {actionType === 'reject' && t('Reject Withdrawal')}
            {actionType === 'complete' && t('Complete Withdrawal')}
          </DialogTitle>
          <DialogDescription>
            {t('User ID')}: {request.inviter_id} | {t('Amount')}:{' '}
            {formatAmount(request.amount)}
          </DialogDescription>
        </DialogHeader>

        {actionType === 'approve' && (
          <form onSubmit={approveForm.handleSubmit(handleApprove)} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="approve-note">{t('Note')} ({t('Optional')})</Label>
              <Textarea
                id="approve-note"
                placeholder={t('Add any notes for this approval')}
                {...approveForm.register('note')}
              />
            </div>

            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose}>
                {t('Cancel')}
              </Button>
              <Button type="submit" disabled={isPending}>
                {isPending ? t('Processing...') : t('Approve')}
              </Button>
            </DialogFooter>
          </form>
        )}

        {actionType === 'reject' && (
          <form onSubmit={rejectForm.handleSubmit(handleReject)} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="reject-reason">
                {t('Reject Reason')} <span className="text-destructive">*</span>
              </Label>
              <Textarea
                id="reject-reason"
                placeholder={t('Please provide a reason for rejection')}
                {...rejectForm.register('reason')}
              />
              {rejectForm.formState.errors.reason && (
                <p className="text-sm text-destructive">
                  {rejectForm.formState.errors.reason.message}
                </p>
              )}
            </div>

            <div className="space-y-2">
              <Label htmlFor="reject-note">{t('Note')} ({t('Optional')})</Label>
              <Textarea
                id="reject-note"
                placeholder={t('Add any additional notes')}
                {...rejectForm.register('note')}
              />
            </div>

            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose}>
                {t('Cancel')}
              </Button>
              <Button type="submit" variant="destructive" disabled={isPending}>
                {isPending ? t('Processing...') : t('Reject')}
              </Button>
            </DialogFooter>
          </form>
        )}

        {actionType === 'complete' && (
          <div className="space-y-4">
            <p className="text-sm text-muted-foreground">
              {t('This will mark the withdrawal as completed. This action cannot be undone.')}
            </p>

            {request.rejectReason && (
              <div className="rounded-md bg-muted p-3">
                <div className="text-sm font-medium">{t('Previous Reject Reason')}:</div>
                <div className="mt-1 text-sm text-muted-foreground">
                  {request.rejectReason}
                </div>
              </div>
            )}

            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose}>
                {t('Cancel')}
              </Button>
              <Button onClick={handleComplete} disabled={isPending}>
                {isPending ? t('Processing...') : t('Complete')}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
