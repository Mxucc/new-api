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

export const ADMIN_PERMISSION_RESOURCES = {
  CHANNEL: 'channel',
};

export const ADMIN_PERMISSION_ACTIONS = {
  READ: 'read',
  OPERATE: 'operate',
  WRITE: 'write',
  SENSITIVE_WRITE: 'sensitive_write',
  SECRET_VIEW: 'secret_view',
};

export const ADMIN_ROLE_KEY = 'admin';

export const EMPTY_PERMISSION_CATALOG = {
  resources: [],
  roles: [],
};

export const getStoredUser = () => {
  try {
    const raw = localStorage.getItem('user');
    return raw ? JSON.parse(raw) : null;
  } catch (error) {
    return null;
  }
};

export const isRootUser = (user) => Number(user?.role) >= 100;

export const getRolePermissionGrants = (catalog, roleKey) => {
  if (!Array.isArray(catalog?.roles)) return {};
  return catalog.roles.find((role) => role.key === roleKey)?.grants || {};
};

export const normalizeAdminPermissions = (value, catalog) => {
  const baseline = getRolePermissionGrants(catalog, ADMIN_ROLE_KEY);
  const normalized = {};

  for (const resource of catalog?.resources || []) {
    const actions = {};
    for (const action of resource.actions || []) {
      actions[action.action] =
        value?.[resource.resource]?.[action.action] ??
        baseline?.[resource.resource]?.[action.action] ??
        false;
    }
    normalized[resource.resource] = actions;
  }

  return normalized;
};

export const hasAdminPermission = (user, resource, action) => {
  if (!user) return false;
  if (isRootUser(user)) return true;
  const matrix =
    user.permissions?.admin_permissions ?? user.admin_permissions ?? {};
  return matrix?.[resource]?.[action] === true;
};
