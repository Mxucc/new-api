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

import { getCurrencyConfig } from '../../helpers/render';

export const ALL_USER_GROUP = '__all__';

export const DEFAULT_PAGE_SIZE = 20;

export function extractData(response, fallback = null) {
  if (!response?.success) {
    throw new Error(response?.message || '请求失败');
  }
  return response.data ?? fallback;
}

const GENERIC_AXIOS_STATUS_MESSAGE = /^Request failed with status code \d+$/;

export function invitationErrorMessage(error, fallback = '请求失败') {
  const responseMessage = error?.response?.data?.message;
  if (typeof responseMessage === 'string' && responseMessage.trim()) {
    return responseMessage;
  }

  const message = error?.message;
  if (
    typeof message === 'string' &&
    message.trim() &&
    !GENERIC_AXIOS_STATUS_MESSAGE.test(message)
  ) {
    return message;
  }

  return fallback;
}

export function getQuotaPerUnitSafe() {
  const raw = parseFloat(localStorage.getItem('quota_per_unit') || '1');
  return Number.isFinite(raw) && raw > 0 ? raw : 1;
}

export function centsToDisplayAmount(cents) {
  const usdAmount = Number(cents || 0) / 100;
  const { type, rate } = getCurrencyConfig();

  if (type === 'TOKENS') {
    return usdAmount * getQuotaPerUnitSafe();
  }

  return usdAmount * (rate || 1);
}

export function displayAmountToCents(amount) {
  const value = Number(amount || 0);
  if (!Number.isFinite(value) || value <= 0) return 0;

  const { type, rate } = getCurrencyConfig();
  let usdAmount = value;

  if (type === 'TOKENS') {
    usdAmount = value / getQuotaPerUnitSafe();
  } else if (rate && rate > 0) {
    usdAmount = value / rate;
  }

  return Math.round(usdAmount * 100);
}

export function formatRebateAmount(cents, digits = 2) {
  const value = centsToDisplayAmount(cents);
  const { symbol, type } = getCurrencyConfig();

  if (type === 'TOKENS') {
    return Number(value || 0).toLocaleString();
  }

  return `${symbol}${Number(value || 0).toFixed(digits)}`;
}

export function formatPercent(rate, digits = 2) {
  if (rate === undefined || rate === null || rate === '') return '';
  const value = Number(rate);
  if (!Number.isFinite(value)) return '';
  return `${(value * 100).toFixed(digits)}%`;
}

export function formatDateTime(value) {
  if (!value) return '-';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '-';
  return date.toLocaleString();
}

export function timestampToDateTime(timestamp) {
  if (!timestamp) return '-';
  const date = new Date(Number(timestamp) * 1000);
  if (Number.isNaN(date.getTime())) return '-';
  return date.toLocaleString();
}

export function formatUser(id, name) {
  return name ? `${name} (#${id})` : `#${id}`;
}

export function toDatetimeLocalValue(value) {
  const date = value ? new Date(value) : new Date();
  if (Number.isNaN(date.getTime())) return '';
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 16);
}

export function addDaysToDatetimeLocal(value, days) {
  const date = value ? new Date(value) : new Date();
  date.setDate(date.getDate() + days);
  return toDatetimeLocalValue(date.toISOString());
}

export function orderTypeLabel(t, orderType) {
  const labels = {
    topup: t('充值'),
    subscription: t('订阅'),
    invite_inviter: t('邀请注册奖励'),
    invite_invitee: t('新用户邀请奖励'),
    other: t('其他'),
  };
  return labels[orderType] || orderType || '-';
}

export function userRebateStatusLabel(t, status, fallbackStatus) {
  const displayStatus =
    status || (fallbackStatus === 'pending' ? 'claimable' : 'paid');
  const labels = {
    estimated: t('预计返利'),
    claimable: t('可返利'),
    paid: t('已返利'),
    waiting_unlock: t('待返利（等待用户首次充值/订阅后解锁）'),
  };
  return labels[displayStatus] || displayStatus || '-';
}

export function adminOrderStatusLabel(t, status) {
  const labels = {
    initializing: t('初始化中'),
    estimated: t('预计返利'),
    claimable: t('可返利'),
    paid: t('已返利'),
    closed: t('已关闭'),
  };
  return labels[status] || status || '-';
}

export function requestStatusLabel(t, status) {
  const labels = {
    pending: t('待处理'),
    approved: t('已通过'),
    rejected: t('已拒绝'),
    completed: t('已完成'),
    requested: t('已申请'),
    closed: t('已关闭'),
  };
  return labels[status] || status || '-';
}

export function isInvitationSignupReward(orderType) {
  return orderType === 'invite_inviter' || orderType === 'invite_invitee';
}

export function getRecordId(record) {
  return Number(record?.localRebateRecordId || 0);
}

export function collectRecordIds(records) {
  return records
    .map((record) => getRecordId(record))
    .filter((id) => Number.isFinite(id) && id > 0);
}

export function groupNameLabel(t, group) {
  return group === ALL_USER_GROUP ? t('全部用户组') : group || '-';
}
