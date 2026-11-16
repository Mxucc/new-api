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

import { StatusBadge } from '@/components/status-badge'
import { formatNumber } from '@/lib/format'

import { iqHasScore, iqVerdictLabelKey, iqVerdictVariant } from '../lib/iq'
import type { ModelIqStatus } from '../types'

/** 智商分数 + 判定：未测过的模型只显示判定，不给分数，避免把 0 分当成结论。 */
export function ModelIqScore(props: { status: ModelIqStatus }) {
  const { t } = useTranslation()
  const scored = iqHasScore(props.status.verdict)
  return (
    <div className='flex items-center gap-2'>
      <span
        className='w-9 shrink-0 text-end text-sm font-semibold tabular-nums'
        data-testid={`model-iq-score-${props.status.group}-${props.status.model_name}`}
      >
        {scored ? formatNumber(props.status.score) : '—'}
      </span>
      <StatusBadge
        label={t(iqVerdictLabelKey(props.status.verdict))}
        variant={iqVerdictVariant(props.status.verdict)}
        copyable={false}
      />
    </div>
  )
}
