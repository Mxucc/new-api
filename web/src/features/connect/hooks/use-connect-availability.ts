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
import { useQuery } from '@tanstack/react-query'

import { CONNECT_ME_QUERY_KEY, getConnectMe } from '../api'

/**
 * Whether this deployment publishes the Connect service. The sidebar hides the
 * entry when the service is absent, the same way the rebate entry follows its own
 * feature status; the page itself shows the unavailable state as a fallback.
 */
export function useConnectAvailability(): boolean {
  const query = useQuery({
    queryKey: CONNECT_ME_QUERY_KEY,
    queryFn: getConnectMe,
    retry: false,
    staleTime: 5 * 60_000,
    gcTime: 5 * 60_000,
    meta: { errorToast: false },
  })
  return query.isSuccess
}
