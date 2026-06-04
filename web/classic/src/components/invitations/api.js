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

import { API } from '../../helpers/api';

const USER_BASE_PATH = '/invitations/api/invitations';
const ADMIN_BASE_PATH = '/invitations/api/admin';
export const INVITATION_FEATURE_STATUS_REFRESH_EVENT =
  'invitation-feature-status-refresh';

const unwrap = (response) => response.data;

export const invitationApi = {
  getStatus() {
    return API.get(`${USER_BASE_PATH}/status`, {
      skipErrorHandler: true,
      disableDuplicate: true,
    }).then(unwrap);
  },

  getMyCode(config) {
    return API.get(`${USER_BASE_PATH}/my-code`, config).then(unwrap);
  },

  getRebateRecords(params) {
    return API.get(`${USER_BASE_PATH}/rebate-records`, { params }).then(unwrap);
  },

  getAvailableRebates() {
    return API.get(`${USER_BASE_PATH}/available-rebates`).then(unwrap);
  },

  requestRebate(data) {
    return API.post(`${USER_BASE_PATH}/rebate-requests`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  getMyRebateRequests(params) {
    return API.get(`${USER_BASE_PATH}/rebate-requests`, { params }).then(
      unwrap,
    );
  },
};

export const invitationAdminApi = {
  getRebateRules() {
    return API.get(`${ADMIN_BASE_PATH}/rebate-rules`).then(unwrap);
  },

  createRebateRule(data) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-rules`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  updateRebateRule(id, data) {
    return API.put(`${ADMIN_BASE_PATH}/rebate-rules/${id}`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  deleteRebateRule(id) {
    return API.delete(`${ADMIN_BASE_PATH}/rebate-rules/${id}`, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  getUserGroups() {
    return API.get(`${ADMIN_BASE_PATH}/user-groups`).then(unwrap);
  },

  getSystemConfig() {
    return API.get(`${ADMIN_BASE_PATH}/system-config`).then(unwrap);
  },

  updateSystemConfig(data) {
    return API.put(`${ADMIN_BASE_PATH}/system-config`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  getRebateRequests(params) {
    return API.get(`${ADMIN_BASE_PATH}/rebate-requests`, { params }).then(
      unwrap,
    );
  },

  approveRebateRequest(id, data) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-requests/${id}/approve`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  rejectRebateRequest(id, data) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-requests/${id}/reject`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  completeRebateRequest(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-requests/${id}/complete`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  undoCompleteRebateRequest(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-requests/${id}/undo-complete`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  resetRebateRequestReview(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-requests/${id}/reset`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  getRebateStats() {
    return API.get(`${ADMIN_BASE_PATH}/rebate-stats`).then(unwrap);
  },

  getRebateRecords(params) {
    return API.get(`${ADMIN_BASE_PATH}/rebate-records`, { params }).then(
      unwrap,
    );
  },

  revokeSignupRewardRecord(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-records/${id}/revoke-signup-reward`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  getRebateOrderRecords(params) {
    return API.get(`${ADMIN_BASE_PATH}/rebate-order-records`, { params }).then(
      unwrap,
    );
  },

  updateRebateOrderRecords(data) {
    return API.patch(`${ADMIN_BASE_PATH}/rebate-order-records`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  closeRebateOrderRecords(data) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-order-records/close`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  reopenRebateOrderRecords(data) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-order-records/reopen`, data, {
      skipErrorHandler: true,
    }).then(unwrap);
  },

  endRebateOrderInitialization(data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-order-records/end-initialization`,
      data,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  extendRebateOrderInitialization(data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-order-records/extend-initialization`,
      data,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  getInvitationRegistrations(params) {
    return API.get(`${ADMIN_BASE_PATH}/invitation-registrations`, {
      params,
    }).then(unwrap);
  },

  generateInvitationInviterReward(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/invitation-registrations/${id}/inviter-reward`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  generateInvitationInviteeReward(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/invitation-registrations/${id}/invitee-reward`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  revokeInvitationInviterReward(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/invitation-registrations/${id}/inviter-reward/revoke`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  revokeInvitationInviteeReward(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/invitation-registrations/${id}/invitee-reward/revoke`,
      undefined,
      { skipErrorHandler: true },
    ).then(unwrap);
  },

  getUserRebateDetails(userId, params) {
    return API.get(`${ADMIN_BASE_PATH}/users/${userId}/rebate-details`, {
      params,
    }).then(unwrap);
  },
};
