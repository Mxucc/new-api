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

  getMyCode() {
    return API.get(`${USER_BASE_PATH}/my-code`).then(unwrap);
  },

  getRebateRecords(params) {
    return API.get(`${USER_BASE_PATH}/rebate-records`, { params }).then(unwrap);
  },

  getAvailableRebates() {
    return API.get(`${USER_BASE_PATH}/available-rebates`).then(unwrap);
  },

  requestRebate(data) {
    return API.post(`${USER_BASE_PATH}/rebate-requests`, data).then(unwrap);
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
    return API.post(`${ADMIN_BASE_PATH}/rebate-rules`, data).then(unwrap);
  },

  updateRebateRule(id, data) {
    return API.put(`${ADMIN_BASE_PATH}/rebate-rules/${id}`, data).then(unwrap);
  },

  deleteRebateRule(id) {
    return API.delete(`${ADMIN_BASE_PATH}/rebate-rules/${id}`).then(unwrap);
  },

  getUserGroups() {
    return API.get(`${ADMIN_BASE_PATH}/user-groups`).then(unwrap);
  },

  getSystemConfig() {
    return API.get(`${ADMIN_BASE_PATH}/system-config`).then(unwrap);
  },

  updateSystemConfig(data) {
    return API.put(`${ADMIN_BASE_PATH}/system-config`, data).then(unwrap);
  },

  getRebateRequests(params) {
    return API.get(`${ADMIN_BASE_PATH}/rebate-requests`, { params }).then(
      unwrap,
    );
  },

  approveRebateRequest(id, data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-requests/${id}/approve`,
      data,
    ).then(unwrap);
  },

  rejectRebateRequest(id, data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-requests/${id}/reject`,
      data,
    ).then(unwrap);
  },

  completeRebateRequest(id) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-requests/${id}/complete`).then(
      unwrap,
    );
  },

  getRebateStats() {
    return API.get(`${ADMIN_BASE_PATH}/rebate-stats`).then(unwrap);
  },

  getRebateOrderRecords(params) {
    return API.get(`${ADMIN_BASE_PATH}/rebate-order-records`, { params }).then(
      unwrap,
    );
  },

  updateRebateOrderRecords(data) {
    return API.patch(`${ADMIN_BASE_PATH}/rebate-order-records`, data).then(
      unwrap,
    );
  },

  closeRebateOrderRecords(data) {
    return API.post(`${ADMIN_BASE_PATH}/rebate-order-records/close`, data).then(
      unwrap,
    );
  },

  reopenRebateOrderRecords(data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-order-records/reopen`,
      data,
    ).then(unwrap);
  },

  endRebateOrderInitialization(data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-order-records/end-initialization`,
      data,
    ).then(unwrap);
  },

  extendRebateOrderInitialization(data) {
    return API.post(
      `${ADMIN_BASE_PATH}/rebate-order-records/extend-initialization`,
      data,
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
    ).then(unwrap);
  },

  generateInvitationInviteeReward(id) {
    return API.post(
      `${ADMIN_BASE_PATH}/invitation-registrations/${id}/invitee-reward`,
    ).then(unwrap);
  },

  getUserRebateDetails(userId) {
    return API.get(`${ADMIN_BASE_PATH}/users/${userId}/rebate-details`).then(
      unwrap,
    );
  },
};
