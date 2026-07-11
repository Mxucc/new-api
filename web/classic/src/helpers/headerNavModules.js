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

export const HEADER_NAV_ACCESS_MODULES = new Set(['pricing', 'rankings']);

const DEFAULT_HEADER_NAV_MODULES = {
  home: true,
  console: true,
  pricing: {
    enabled: true,
    requireAuth: false,
  },
  rankings: {
    enabled: true,
    requireAuth: false,
  },
  docs: true,
  about: true,
};

const cloneDefaults = () => structuredClone(DEFAULT_HEADER_NAV_MODULES);

export const parseHeaderNavBoolean = (value, fallback) => {
  if (typeof value === 'boolean') return value;
  if (typeof value === 'number') {
    if (value === 1) return true;
    if (value === 0) return false;
    return fallback;
  }
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (normalized === 'true' || normalized === '1') return true;
    if (normalized === 'false' || normalized === '0') return false;
  }
  return fallback;
};

const normalizeAccessModule = (value, fallback) => {
  if (
    typeof value === 'boolean' ||
    typeof value === 'number' ||
    typeof value === 'string'
  ) {
    return {
      enabled: parseHeaderNavBoolean(value, fallback.enabled),
      requireAuth: fallback.requireAuth,
    };
  }

  if (!value || typeof value !== 'object') {
    return { ...fallback };
  }

  return {
    enabled: parseHeaderNavBoolean(value.enabled, fallback.enabled),
    requireAuth: parseHeaderNavBoolean(value.requireAuth, fallback.requireAuth),
  };
};

export const createDefaultHeaderNavModules = cloneDefaults;

export const parseHeaderNavModules = (raw) => {
  const defaults = cloneDefaults();
  if (!raw) return defaults;

  try {
    const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
    if (!parsed || typeof parsed !== 'object') return defaults;

    for (const [key, value] of Object.entries(parsed)) {
      if (HEADER_NAV_ACCESS_MODULES.has(key)) {
        defaults[key] = normalizeAccessModule(value, defaults[key]);
        continue;
      }

      const fallback = defaults[key];
      if (
        typeof fallback === 'boolean' ||
        typeof value === 'boolean' ||
        typeof value === 'number' ||
        typeof value === 'string'
      ) {
        defaults[key] = parseHeaderNavBoolean(
          value,
          typeof fallback === 'boolean' ? fallback : true,
        );
      }
    }

    return defaults;
  } catch (error) {
    console.error('Failed to parse header navigation modules:', error);
    return defaults;
  }
};

export const getHeaderNavModuleAccess = (modules, moduleKey) => {
  const value = modules?.[moduleKey];
  if (HEADER_NAV_ACCESS_MODULES.has(moduleKey)) {
    return normalizeAccessModule(value, DEFAULT_HEADER_NAV_MODULES[moduleKey]);
  }

  return {
    enabled: value === true,
    requireAuth: false,
  };
};
