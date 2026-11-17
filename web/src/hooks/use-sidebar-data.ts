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
import {
  Activity,
  Box,
  Brain,
  ClipboardList,
  CreditCard,
  FileText,
  FlaskConical,
  Gauge,
  Gift,
  HeartPulse,
  Key,
  LayoutDashboard,
  Link2,
  ListTodo,
  MessageSquare,
  PlugZap,
  Radio,
  ServerCog,
  Settings,
  ShieldCheck,
  Ticket,
  User,
  Users,
  Wallet,
  Workflow,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { SidebarBadges, SidebarData } from '@/components/layout/types'
import { useAiBridgeIdentity } from '@/features/ai-bridge/hooks/use-ai-bridge-console'
import { useConnectAvailability } from '@/features/connect/hooks/use-connect-availability'
import { useInvitationFeatureStatus } from '@/features/invitations/hooks/use-invitation-feature-status'
import { useStatus } from '@/hooks/use-status'
import { ROLE } from '@/lib/roles'
import { useAuthStore } from '@/stores/auth-store'

/**
 * Root navigation groups for the application sidebar.
 *
 * These are shown when the URL does not match any nested sidebar view
 * registered in `layout/lib/sidebar-view-registry.ts`.
 */
export function useSidebarData(badges: SidebarBadges = {}): SidebarData {
  const { t } = useTranslation()
  const invitationFeature = useInvitationFeatureStatus()
  const connectAvailable = useConnectAvailability()
  const role = useAuthStore((state) => state.auth.user?.role ?? 0)
  const { status } = useStatus()
  // Only an administrator session can open the console, and only that session
  // should probe it; everyone else keeps the entry out of the sidebar.
  const aiBridgeAvailable = useAiBridgeIdentity(role >= ROLE.ADMIN).isSuccess

  return {
    navGroups: [
      {
        id: 'chat',
        title: t('Chat'),
        items: [
          {
            title: t('Playground'),
            url: '/playground',
            icon: FlaskConical,
          },
          {
            title: t('Chat'),
            icon: MessageSquare,
            type: 'chat-presets',
          },
        ],
      },
      {
        id: 'general',
        title: t('General'),
        items: [
          {
            title: t('Overview'),
            url: '/dashboard/overview',
            icon: Activity,
          },
          {
            title: t('Dashboard'),
            url: '/dashboard/models',
            icon: LayoutDashboard,
          },
          // 没配探测目标时这页对普通用户没有内容：入口先不出现，别让人点进去看空页。
          ...(role >= ROLE.ADMIN || status?.model_iq_configured
            ? [
                {
                  title: t('Model IQ'),
                  url: '/model-iq',
                  icon: Brain,
                },
              ]
            : []),
          {
            title: t('Service Status'),
            url: '/service-status',
            icon: HeartPulse,
          },
          {
            title: t('API Keys'),
            url: '/keys',
            icon: Key,
          },
          {
            title: t('Usage Logs'),
            url: '/usage-logs/common',
            icon: FileText,
          },
          {
            title: t('Audit Logs'),
            url: '/usage-logs/audit',
            icon: ClipboardList,
          },
          {
            title: t('Task Logs'),
            url: '/usage-logs/task',
            activeUrls: ['/usage-logs/drawing'],
            configUrls: ['/usage-logs/drawing', '/usage-logs/task'],
            icon: ListTodo,
          },
        ],
      },
      {
        id: 'personal',
        title: t('Personal'),
        items: [
          {
            title: t('Wallet'),
            url: '/wallet',
            icon: Wallet,
          },
          ...(invitationFeature.userVisible
            ? [
                {
                  title: t('Invitations'),
                  url: '/invitations',
                  icon: Gift,
                },
              ]
            : []),
          ...(connectAvailable
            ? [
                {
                  title: t('Connect'),
                  url: '/connect',
                  icon: Link2,
                },
              ]
            : []),
          {
            title: t('Profile'),
            url: '/profile',
            icon: User,
          },
          {
            title: t('Security & Access'),
            url: '/security',
            icon: ShieldCheck,
          },
        ],
      },
      {
        id: 'admin',
        title: t('Admin'),
        items: [
          {
            title: t('Channels'),
            url: '/channels',
            icon: Radio,
          },
          {
            title: t('Models'),
            url: '/models/metadata',
            icon: Box,
          },
          {
            title: t('Users'),
            url: '/users',
            icon: Users,
          },
          {
            title: t('Redemption Codes'),
            url: '/redemption-codes',
            icon: Ticket,
          },
          {
            title: t('Subscriptions'),
            url: '/subscriptions',
            icon: CreditCard,
          },
          ...(invitationFeature.available
            ? [
                {
                  title: t('Rebate Management'),
                  url: '/invitations/admin',
                  icon: Gift,
                },
              ]
            : []),
          ...(aiBridgeAvailable
            ? [
                {
                  title: t('AI Bridge'),
                  url: '/ai-bridge',
                  icon: Gauge,
                },
              ]
            : []),
          {
            title: t('System Info'),
            url: '/system-info',
            icon: ServerCog,
            requiredRole: ROLE.SUPER_ADMIN,
          },
          {
            title: t('Plugins'),
            url: '/task-plugins',
            icon: PlugZap,
            requiredRole: ROLE.SUPER_ADMIN,
          },
          {
            title: t('Request processing'),
            url: '/request-processing',
            icon: Workflow,
            requiredRole: ROLE.SUPER_ADMIN,
          },
          {
            title: t('System Settings'),
            url: '/system-settings/site',
            activeUrls: ['/system-settings'],
            icon: Settings,
            requiredRole: ROLE.SUPER_ADMIN,
            // Model pricing lives in System Settings.
            badge: badges.modelPricing,
          },
        ],
      },
    ],
  }
}
