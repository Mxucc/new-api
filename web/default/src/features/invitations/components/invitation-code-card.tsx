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
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { QRCodeSVG } from 'qrcode.react'
import { Copy, QrCode } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { InvitationStats } from '../types'

interface InvitationCodeCardProps {
  stats: InvitationStats | null
  loading: boolean
}

export function InvitationCodeCard({ stats, loading }: InvitationCodeCardProps) {
  const { t } = useTranslation()
  const [qrDialogOpen, setQrDialogOpen] = useState(false)

  // 生成邀请链接
  const invitationLink = useMemo(() => {
    if (!stats?.invitationCode) return ''
    const baseUrl = window.location.origin
    return `${baseUrl}/?aff=${stats.invitationCode}`
  }, [stats])

  // 复制邀请码
  const handleCopyCode = useCallback(() => {
    if (!stats?.invitationCode) return
    navigator.clipboard.writeText(stats.invitationCode)
    toast.success(t('Invitation code copied'))
  }, [stats, t])

  // 复制邀请链接
  const handleCopyLink = useCallback(() => {
    if (!invitationLink) return
    navigator.clipboard.writeText(invitationLink)
    toast.success(t('Invitation link copied'))
  }, [invitationLink, t])

  if (loading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>{t('My Invitation Code')}</CardTitle>
          <CardDescription>{t('Share your invitation code to earn rebates')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center py-8">
            <div className="text-muted-foreground">{t('Loading...')}</div>
          </div>
        </CardContent>
      </Card>
    )
  }

  if (!stats) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>{t('My Invitation Code')}</CardTitle>
          <CardDescription>{t('Share your invitation code to earn rebates')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center py-8">
            <div className="text-muted-foreground">{t('Failed to load invitation code')}</div>
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>{t('My Invitation Code')}</CardTitle>
          <CardDescription>{t('Share your invitation code to earn rebates')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          {/* 邀请码展示 */}
          <div className="space-y-2">
            <Label>{t('Invitation Code')}</Label>
            <div className="flex gap-2">
              <div className="flex-1 rounded-lg border bg-muted/50 px-4 py-3">
                <div className="text-2xl font-bold tracking-wider">
                  {stats.invitationCode}
                </div>
              </div>
              <Button
                variant="outline"
                size="icon"
                onClick={handleCopyCode}
                className="shrink-0"
              >
                <Copy className="size-4" />
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={() => setQrDialogOpen(true)}
                className="shrink-0"
              >
                <QrCode className="size-4" />
              </Button>
            </div>
          </div>

          {/* 邀请链接 */}
          <div className="space-y-2">
            <Label>{t('Invitation Link')}</Label>
            <div className="flex gap-2">
              <Input
                value={invitationLink}
                readOnly
                className="flex-1 font-mono text-sm"
              />
              <Button
                variant="outline"
                size="icon"
                onClick={handleCopyLink}
                className="shrink-0"
              >
                <Copy className="size-4" />
              </Button>
            </div>
          </div>

          {/* 统计卡片 */}
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <div className="rounded-lg border bg-card p-4">
              <div className="text-sm text-muted-foreground">{t('Invited Users')}</div>
              <div className="mt-1 text-2xl font-bold">{stats.invitedCount}</div>
            </div>
            <div className="rounded-lg border bg-card p-4">
              <div className="text-sm text-muted-foreground">{t('Total Rebate')}</div>
              <div className="mt-1 text-2xl font-bold">
                ${(stats.totalRebate / 100).toFixed(2)}
              </div>
            </div>
            <div className="rounded-lg border bg-card p-4">
              <div className="text-sm text-muted-foreground">{t('Completed Rebate')}</div>
              <div className="mt-1 text-2xl font-bold">
                ${(stats.completedRebate / 100).toFixed(2)}
              </div>
            </div>
            <div className="rounded-lg border bg-card p-4">
              <div className="text-sm text-muted-foreground">{t('Pending Withdrawal')}</div>
              <div className="mt-1 text-2xl font-bold">
                ${(stats.pendingWithdrawal / 100).toFixed(2)}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* 二维码对话框 */}
      <Dialog open={qrDialogOpen} onOpenChange={setQrDialogOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t('Invitation QR Code')}</DialogTitle>
            <DialogDescription>
              {t('Scan this QR code to register with your invitation code')}
            </DialogDescription>
          </DialogHeader>
          <div className="flex justify-center py-6">
            <div className="rounded-lg border bg-white p-4">
              <QRCodeSVG value={invitationLink} size={200} level="H" />
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
