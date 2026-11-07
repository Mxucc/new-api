/*
Copyright (C) 2025 QuantumNous

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

const EXTERNAL_AUTH_EXCHANGE_PATHS = [
  ['/connect', '/connect/auth/exchange'],
  ['/ai-bridge', '/ai-bridge/auth/exchange'],
  ['/jobs-files', '/jobs-files/auth/exchange'],
  ['/jobs-flies', '/jobs-flies/auth/exchange'],
];

export function getExternalAuthExchangePath(target, origin) {
  let trustedOrigin;
  let targetUrl;

  try {
    trustedOrigin = new URL(origin);
    targetUrl = new URL(target, trustedOrigin.origin);
  } catch {
    return null;
  }

  if (
    !['http:', 'https:'].includes(trustedOrigin.protocol) ||
    targetUrl.origin !== trustedOrigin.origin
  ) {
    return null;
  }

  const match = EXTERNAL_AUTH_EXCHANGE_PATHS.find(
    ([prefix]) =>
      targetUrl.pathname === prefix ||
      targetUrl.pathname.startsWith(`${prefix}/`),
  );

  return match?.[1] ?? null;
}

function getDashboardAuthHeaders() {
  let user;

  try {
    user = JSON.parse(window.localStorage.getItem('user'));
  } catch {
    user = null;
  }

  if (!user?.token || typeof user.token !== 'string') {
    throw new Error('登录凭证不可用，请重新登录后重试');
  }

  return { Authorization: `Bearer ${user.token}` };
}

export async function exchangeExternalAuthForRedirect(
  target,
  origin = window.location.origin,
) {
  const exchangePath = getExternalAuthExchangePath(target, origin);
  if (!exchangePath) return;

  const response = await fetch(exchangePath, {
    method: 'POST',
    headers: getDashboardAuthHeaders(),
    credentials: 'same-origin',
    cache: 'no-store',
  });

  if (!response.ok) {
    throw new Error(`外部应用鉴权失败（HTTP ${response.status}）`);
  }
}
