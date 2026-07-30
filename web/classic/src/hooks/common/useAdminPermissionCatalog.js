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

import { useEffect, useState } from 'react';
import { API } from '../../helpers';
import { EMPTY_PERMISSION_CATALOG } from '../../helpers/adminPermissions';

let cachedCatalog = null;
let catalogRequest = null;

const requestCatalog = async () => {
  if (cachedCatalog) return cachedCatalog;
  if (!catalogRequest) {
    catalogRequest = API.get('/api/authz/catalog', {
      skipErrorHandler: true,
    })
      .then((res) => {
        if (!res?.data?.success) {
          throw new Error(res?.data?.message || 'Permission catalog failed');
        }
        cachedCatalog = {
          resources: Array.isArray(res.data?.data?.resources)
            ? res.data.data.resources
            : [],
          roles: Array.isArray(res.data?.data?.roles)
            ? res.data.data.roles
            : [],
        };
        return cachedCatalog;
      })
      .finally(() => {
        catalogRequest = null;
      });
  }
  return catalogRequest;
};

export const useAdminPermissionCatalog = (enabled = true) => {
  const [catalog, setCatalog] = useState(
    cachedCatalog || EMPTY_PERMISSION_CATALOG,
  );
  const [loading, setLoading] = useState(Boolean(enabled && !cachedCatalog));
  const [error, setError] = useState('');

  useEffect(() => {
    if (!enabled) {
      setLoading(false);
      return undefined;
    }

    let active = true;
    setLoading(!cachedCatalog);
    setError('');
    requestCatalog()
      .then((nextCatalog) => {
        if (active) setCatalog(nextCatalog);
      })
      .catch((requestError) => {
        if (active) {
          setError(
            requestError?.response?.data?.message ||
              requestError?.message ||
              'Permission catalog failed',
          );
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [enabled]);

  return { catalog, loading, error };
};

export default useAdminPermissionCatalog;
